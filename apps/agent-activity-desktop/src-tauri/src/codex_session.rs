use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use activity_protocol::{ActivityEvent, EventKind, SourceKind, SCHEMA_VERSION};
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use serde_json::Value;

const INITIAL_TAIL_BYTES: u64 = 1024 * 1024;
const RECENT_FILE_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
struct SessionMetadata {
    session_id: String,
    project: Option<String>,
}

struct TrackedFile {
    offset: u64,
    session: SessionMetadata,
    initial_scan: bool,
}

pub struct CodexSessionWatcher {
    root: PathBuf,
    files: HashMap<PathBuf, TrackedFile>,
}

impl CodexSessionWatcher {
    pub fn from_environment() -> Option<Self> {
        let root = sessions_dir()?;
        root.is_dir().then(|| Self::new(root))
    }

    fn new(root: PathBuf) -> Self {
        Self {
            root,
            files: HashMap::new(),
        }
    }

    pub fn poll(&mut self) -> Vec<ActivityEvent> {
        let paths = discover_jsonl_files(&self.root);
        let present: HashSet<_> = paths.iter().cloned().collect();
        self.files.retain(|path, _| present.contains(path));

        let mut events = Vec::new();
        for path in paths {
            if !self.files.contains_key(&path) {
                if let Some(file) = track_file(&path) {
                    self.files.insert(path.clone(), file);
                } else {
                    continue;
                }
            }
            let Some(file) = self.files.get_mut(&path) else {
                continue;
            };
            events.extend(read_new_events(&path, file));
        }
        events
    }
}

pub fn sessions_dir() -> Option<PathBuf> {
    if let Some(home) = env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(home).join("sessions"));
    }
    BaseDirs::new().map(|dirs| dirs.home_dir().join(".codex/sessions"))
}

fn discover_jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut directories = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn track_file(path: &Path) -> Option<TrackedFile> {
    let metadata = fs::metadata(path).ok()?;
    let session = read_session_metadata(path)?;
    let recent = metadata
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map_or(true, |age| age <= RECENT_FILE_AGE);
    let offset = if recent {
        initial_tail_offset(path, metadata.len()).unwrap_or(metadata.len())
    } else {
        metadata.len()
    };
    Some(TrackedFile {
        offset,
        session,
        initial_scan: true,
    })
}

fn read_session_metadata(path: &Path) -> Option<SessionMetadata> {
    let mut line = String::new();
    BufReader::new(File::open(path).ok()?)
        .read_line(&mut line)
        .ok()?;
    let record: Value = serde_json::from_str(&line).ok()?;
    if record.get("type")?.as_str()? != "session_meta" {
        return None;
    }
    let payload = record.get("payload")?;
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("id"))?
        .as_str()?
        .to_owned();
    let project = payload
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(project_name);
    Some(SessionMetadata {
        session_id,
        project,
    })
}

fn initial_tail_offset(path: &Path, length: u64) -> Option<u64> {
    if length <= INITIAL_TAIL_BYTES {
        return Some(0);
    }
    let start = length - INITIAL_TAIL_BYTES;
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(start - 1)).ok()?;
    let mut previous = [0_u8; 1];
    file.read_exact(&mut previous).ok()?;
    if previous[0] == b'\n' {
        return Some(start);
    }
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut reader = BufReader::new(file);
    let mut partial_line = Vec::new();
    reader.read_until(b'\n', &mut partial_line).ok()?;
    reader.stream_position().ok()
}

fn read_new_events(path: &Path, tracked: &mut TrackedFile) -> Vec<ActivityEvent> {
    let Ok(metadata) = fs::metadata(path) else {
        return Vec::new();
    };
    if metadata.len() < tracked.offset {
        tracked.offset = initial_tail_offset(path, metadata.len()).unwrap_or(metadata.len());
        tracked.initial_scan = false;
    }
    if metadata.len() == tracked.offset {
        tracked.initial_scan = false;
        return Vec::new();
    }

    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    if file.seek(SeekFrom::Start(tracked.offset)).is_err() {
        return Vec::new();
    }
    let mut appended = Vec::new();
    if file.read_to_end(&mut appended).is_err() {
        return Vec::new();
    }
    let Some(last_newline) = appended.iter().rposition(|byte| *byte == b'\n') else {
        return Vec::new();
    };
    let complete_length = last_newline + 1;
    tracked.offset = tracked.offset.saturating_add(complete_length as u64);

    let events: Vec<_> = appended[..complete_length]
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| parse_session_event(line, &tracked.session))
        .collect();

    if !tracked.initial_scan {
        return events;
    }
    tracked.initial_scan = false;

    // The first poll may read the tail of a long-lived log. Only replay the
    // activity after its latest terminal event; older completed turns must not
    // recreate sessions that the user dismissed while the app was closed.
    let last_terminal = events
        .iter()
        .rposition(|event| matches!(event.kind, EventKind::RunCompleted | EventKind::RunAborted));
    events
        .into_iter()
        .skip(last_terminal.map_or(0, |index| index + 1))
        .collect()
}

fn parse_session_event(line: &[u8], session: &SessionMetadata) -> Option<ActivityEvent> {
    let record: Value = serde_json::from_slice(line).ok()?;
    if record.get("type")?.as_str()? != "event_msg" {
        return None;
    }
    let payload = record.get("payload")?;
    let payload_type = payload.get("type")?.as_str()?;
    let kind = match payload_type {
        "task_started" | "task_start" | "turn_started" | "turn_start" | "user_message" => {
            EventKind::UserPrompted
        }
        "agent_message" => EventKind::ModelWorking,
        "turn_aborted" => EventKind::RunAborted,
        // Codex CLI writes the terminal marker as task_complete. Older/newer
        // builds have used the turn_* and task_completed spellings as well.
        "task_complete" | "task_completed" | "turn_complete" | "turn_completed" => {
            EventKind::RunCompleted
        }
        _ => return None,
    };
    let occurred_at = record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    let turn_id = payload
        .get("turn_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let identity = turn_id
        .clone()
        .unwrap_or_else(|| occurred_at.timestamp_micros().to_string());
    let mut attributes = BTreeMap::new();
    if let Some(project) = &session.project {
        attributes.insert("project".into(), Value::String(project.clone()));
    }
    Some(ActivityEvent {
        schema_version: SCHEMA_VERSION.into(),
        event_id: format!(
            "codex-session:{}:{payload_type}:{identity}",
            session.session_id
        ),
        provider: "codex".into(),
        adapter_id: "builtin.codex.session-log".into(),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        source_kind: SourceKind::SessionLog,
        instance_id: "codex-local".into(),
        session_id: session.session_id.clone(),
        turn_id,
        correlation_id: None,
        sequence: None,
        kind,
        occurred_at,
        observed_at: Utc::now(),
        tool: None,
        attributes,
    })
}

fn project_name(path: &str) -> Option<String> {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
    };

    use uuid::Uuid;

    use super::*;

    #[test]
    fn parses_structured_turn_aborted_with_session_identity() {
        let event = parse_session_event(
            &serde_json::to_vec(&serde_json::json!({
                "timestamp": "2026-07-15T11:26:20.000Z",
                "type": "event_msg",
                "payload": {"type": "turn_aborted", "turn_id": "turn-1"}
            }))
            .unwrap(),
            &SessionMetadata {
                session_id: "session-1".into(),
                project: Some("project-a".into()),
            },
        )
        .unwrap();

        assert_eq!(event.kind, EventKind::RunAborted);
        assert_eq!(event.session_id, "session-1");
        assert_eq!(event.instance_id, "codex-local");
        assert_eq!(event.source_kind, SourceKind::SessionLog);
        assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(
            event.attributes.get("project"),
            Some(&Value::String("project-a".into()))
        );
    }

    #[test]
    fn parses_codex_task_complete_as_run_completed() {
        let event = parse_session_event(
            &serde_json::to_vec(&serde_json::json!({
                "timestamp": "2026-07-15T11:26:20.000Z",
                "type": "event_msg",
                "payload": {"type": "task_complete", "turn_id": "turn-1"}
            }))
            .unwrap(),
            &SessionMetadata {
                session_id: "session-1".into(),
                project: Some("project-a".into()),
            },
        )
        .unwrap();

        assert_eq!(event.kind, EventKind::RunCompleted);
        assert_eq!(
            event.event_id,
            "codex-session:session-1:task_complete:turn-1"
        );
        assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(
            event.attributes.get("project"),
            Some(&Value::String("project-a".into()))
        );
    }

    #[test]
    fn session_log_start_recreates_a_working_session() {
        let root = test_root();
        let path = root.join("one.jsonl");
        write_session_with_event(
            &path,
            "session-1",
            "/work/project-a",
            "task_started",
            Some("turn-1"),
        );

        let events = CodexSessionWatcher::new(root.clone()).poll();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::UserPrompted);

        let mut engine = activity_core::ActivityEngine::new();
        engine.apply(events[0].clone()).unwrap();
        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(
            snapshot.global.status,
            activity_protocol::SessionStatus::Working
        );
        assert_eq!(snapshot.global.session_id.as_deref(), Some("session-1"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn initial_scan_ignores_completed_history_but_keeps_current_turn() {
        let root = test_root();
        let path = root.join("one.jsonl");
        write_session_with_event(
            &path,
            "session-1",
            "/work/project-a",
            "task_complete",
            Some("turn-old"),
        );
        append_event(&path, "task_started", "turn-current");

        let events = CodexSessionWatcher::new(root.clone()).poll();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::UserPrompted);
        assert_eq!(events[0].turn_id.as_deref(), Some("turn-current"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn polls_multiple_codex_sessions_independently() {
        let root = test_root();
        write_session_with_event(
            &root.join("one.jsonl"),
            "session-1",
            "/work/a",
            "task_started",
            Some("turn-1"),
        );
        write_session_with_event(
            &root.join("two.jsonl"),
            "session-2",
            "/work/b",
            "task_started",
            Some("turn-2"),
        );

        let mut session_ids: Vec<_> = CodexSessionWatcher::new(root.clone())
            .poll()
            .into_iter()
            .map(|event| event.session_id)
            .collect();
        session_ids.sort();

        assert_eq!(session_ids, ["session-1", "session-2"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn emits_each_appended_abort_once() {
        let root = test_root();
        let path = root.join("one.jsonl");
        write_session(&path, "session-1", "/work/a", None);
        let mut watcher = CodexSessionWatcher::new(root.clone());
        assert!(watcher.poll().is_empty());

        append_abort(&path, "turn-new");
        let events = watcher.poll();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].turn_id.as_deref(), Some("turn-new"));
        assert!(watcher.poll().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    fn test_root() -> PathBuf {
        let root = env::temp_dir().join(format!("codex-session-watcher-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_session(path: &Path, session_id: &str, cwd: &str, turn_id: Option<&str>) {
        write_session_with_event(path, session_id, cwd, "turn_aborted", turn_id);
    }

    fn write_session_with_event(
        path: &Path,
        session_id: &str,
        cwd: &str,
        event_type: &str,
        turn_id: Option<&str>,
    ) {
        let mut file = File::create(path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "timestamp": "2026-07-15T11:26:20.000Z",
                "type": "session_meta",
                "payload": {"session_id": session_id, "cwd": cwd}
            })
        )
        .unwrap();
        if let Some(turn_id) = turn_id {
            write_event(&mut file, event_type, turn_id);
        }
    }

    fn append_abort(path: &Path, turn_id: &str) {
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        write_event(&mut file, "turn_aborted", turn_id);
    }

    fn append_event(path: &Path, event_type: &str, turn_id: &str) {
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        write_event(&mut file, event_type, turn_id);
    }

    fn write_event(file: &mut File, event_type: &str, turn_id: &str) {
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "timestamp": "2026-07-15T11:26:29.276Z",
                "type": "event_msg",
                "payload": {
                    "type": event_type,
                    "turn_id": turn_id,
                    "reason": "interrupted"
                }
            })
        )
        .unwrap();
    }
}
