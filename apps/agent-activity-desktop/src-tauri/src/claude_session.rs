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
const METADATA_SCAN_BYTES: usize = 256 * 1024;
const RECENT_FILE_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
struct SessionMetadata {
    session_id: String,
    project: Option<String>,
}

struct TrackedFile {
    offset: u64,
    session: SessionMetadata,
}

pub struct ClaudeSessionWatcher {
    root: PathBuf,
    files: HashMap<PathBuf, TrackedFile>,
}

impl ClaudeSessionWatcher {
    pub fn from_environment() -> Option<Self> {
        let root = projects_dir()?;
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
            if let Some(file) = self.files.get_mut(&path) {
                events.extend(read_new_events(&path, file));
            }
        }
        events
    }
}

fn projects_dir() -> Option<PathBuf> {
    if let Some(config) = env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(config).join("projects"));
    }
    BaseDirs::new().map(|dirs| dirs.home_dir().join(".claude/projects"))
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
    Some(TrackedFile { offset, session })
}

fn read_session_metadata(path: &Path) -> Option<SessionMetadata> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let mut scanned = 0_usize;
    let mut session_id = None;
    let mut project = None;
    while scanned < METADATA_SCAN_BYTES && project.is_none() {
        let mut line = Vec::new();
        let bytes = reader.read_until(b'\n', &mut line).ok()?;
        if bytes == 0 {
            break;
        }
        scanned = scanned.saturating_add(bytes);
        let Ok(record) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        session_id = session_id.or_else(|| {
            record
                .get("sessionId")
                .or_else(|| record.get("session_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
        project = record
            .get("cwd")
            .and_then(Value::as_str)
            .and_then(project_name);
    }
    let session_id = session_id.or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_owned)
    })?;
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
    }
    if metadata.len() == tracked.offset {
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
    appended[..complete_length]
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| parse_rejection(line, &tracked.session))
        .collect()
}

fn parse_rejection(line: &[u8], session: &SessionMetadata) -> Option<ActivityEvent> {
    let record: Value = serde_json::from_slice(line).ok()?;
    if record.get("type")?.as_str()? != "user" {
        return None;
    }
    let explicitly_rejected = record
        .get("toolDenialKind")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "user-rejected")
        || record
            .get("toolUseResult")
            .and_then(Value::as_str)
            .is_some_and(|result| result == "User rejected tool use");
    if !explicitly_rejected {
        return None;
    }
    let occurred_at = record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    let correlation_id = record
        .pointer("/message/content")
        .and_then(Value::as_array)
        .and_then(|content| {
            content.iter().find_map(|item| {
                (item.get("type").and_then(Value::as_str) == Some("tool_result"))
                    .then(|| item.get("tool_use_id").and_then(Value::as_str))
                    .flatten()
                    .map(str::to_owned)
            })
        });
    let identity = record
        .get("uuid")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| correlation_id.clone())
        .unwrap_or_else(|| occurred_at.timestamp_micros().to_string());
    let session_id = record
        .get("sessionId")
        .or_else(|| record.get("session_id"))
        .and_then(Value::as_str)
        .unwrap_or(&session.session_id)
        .to_owned();
    let mut attributes = BTreeMap::new();
    if let Some(project) = &session.project {
        attributes.insert("project".into(), Value::String(project.clone()));
    }
    Some(ActivityEvent {
        schema_version: SCHEMA_VERSION.into(),
        event_id: format!("claude-session:{session_id}:tool-rejected:{identity}"),
        provider: "claude".into(),
        adapter_id: "builtin.claude.session-log".into(),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        source_kind: SourceKind::SessionLog,
        instance_id: "claude-local".into(),
        session_id,
        turn_id: None,
        correlation_id,
        sequence: None,
        kind: EventKind::RunAborted,
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
    fn parses_structured_user_rejection() {
        let root = test_root();
        let path = root.join("session-1.jsonl");
        write_session(&path, "session-1", Some("tool-1"));

        let events = ClaudeSessionWatcher::new(root.clone()).poll();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::RunAborted);
        assert_eq!(events[0].session_id, "session-1");
        assert_eq!(events[0].instance_id, "claude-local");
        assert_eq!(events[0].correlation_id.as_deref(), Some("tool-1"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn polls_multiple_claude_sessions_independently() {
        let root = test_root();
        write_session(&root.join("one.jsonl"), "session-1", Some("tool-1"));
        write_session(&root.join("two.jsonl"), "session-2", Some("tool-2"));

        let mut ids: Vec<_> = ClaudeSessionWatcher::new(root.clone())
            .poll()
            .into_iter()
            .map(|event| event.session_id)
            .collect();
        ids.sort();

        assert_eq!(ids, ["session-1", "session-2"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn emits_each_appended_rejection_once() {
        let root = test_root();
        let path = root.join("session-1.jsonl");
        write_session(&path, "session-1", None);
        let mut watcher = ClaudeSessionWatcher::new(root.clone());
        assert!(watcher.poll().is_empty());

        append_rejection(&path, "session-1", "tool-new");
        assert_eq!(watcher.poll().len(), 1);
        assert!(watcher.poll().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    fn test_root() -> PathBuf {
        let root = env::temp_dir().join(format!("claude-session-watcher-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_session(path: &Path, session_id: &str, rejection: Option<&str>) {
        let mut file = File::create(path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": "/work/project-a",
                "timestamp": "2026-07-15T12:01:19.215Z",
                "message": {"content": "create a directory"}
            })
        )
        .unwrap();
        if let Some(tool_id) = rejection {
            write_rejection(&mut file, session_id, tool_id);
        }
    }

    fn append_rejection(path: &Path, session_id: &str, tool_id: &str) {
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        write_rejection(&mut file, session_id, tool_id);
    }

    fn write_rejection(file: &mut File, session_id: &str, tool_id: &str) {
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "user",
                "sessionId": session_id,
                "cwd": "/work/project-a",
                "uuid": format!("rejection-{tool_id}"),
                "timestamp": "2026-07-15T12:01:43.276Z",
                "toolDenialKind": "user-rejected",
                "toolUseResult": "User rejected tool use",
                "message": {"content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_id,
                    "is_error": true,
                    "content": "redacted"
                }]}
            })
        )
        .unwrap();
    }
}
