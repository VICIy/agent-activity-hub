use std::{
    collections::BTreeMap,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use activity_ipc::{send_event, Endpoint, Response};
use activity_protocol::{ActivityEvent, EventKind, SourceKind, ToolDescriptor, SCHEMA_VERSION};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(35);
const RETRY_TIMEOUT: Duration = Duration::from_millis(80);
const MAX_STDIN_BYTES: u64 = 256 * 1024;
const MAX_SPOOL_BYTES: u64 = 1024 * 1024;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Provider hooks must never fail because the status application is unavailable.
    let _ = run().await;
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let event = if args.first().map(String::as_str) == Some("emit") {
        parse_emit_args(args[1..].to_vec())?
    } else {
        let provider = flag_value(&args, "--provider").unwrap_or_else(|| "codex".to_string());
        let mut input = String::new();
        io::stdin()
            .take(MAX_STDIN_BYTES)
            .read_to_string(&mut input)
            .context("read hook payload")?;
        let mut payload: Value = serde_json::from_str(&input).context("parse hook payload")?;
        inject_event_name(&mut payload, flag_value(&args, "--event"))?;
        map_hook_event(&provider, payload)?
    };
    event.validate().context("validate standard event")?;

    let endpoint = Endpoint::current_user()?;
    if deliver(&endpoint, event.clone(), CONNECT_TIMEOUT)
        .await
        .is_ok()
    {
        return Ok(());
    }

    try_start_background();
    tokio::time::sleep(Duration::from_millis(25)).await;
    if deliver(&endpoint, event.clone(), RETRY_TIMEOUT)
        .await
        .is_ok()
    {
        return Ok(());
    }

    append_spool(&event).await
}

async fn deliver(endpoint: &Endpoint, event: ActivityEvent, timeout: Duration) -> Result<()> {
    let response = send_event(endpoint, event, timeout).await?;
    ensure_accepted(response)
}

fn ensure_accepted(response: Response) -> Result<()> {
    anyhow::ensure!(response.accepted, "IPC rejected event: {}", response.code);
    Ok(())
}

fn parse_emit_args(args: Vec<String>) -> Result<ActivityEvent> {
    let value = |name: &str| -> Result<String> {
        let position = args
            .iter()
            .position(|argument| argument == name)
            .with_context(|| format!("missing {name}"))?;
        args.get(position + 1)
            .cloned()
            .with_context(|| format!("missing value for {name}"))
    };
    let provider = value("--provider")?;
    let session_id = value("--session")?;
    let instance_id = args
        .iter()
        .position(|argument| argument == "--instance")
        .and_then(|position| args.get(position + 1).cloned())
        .unwrap_or_else(|| "local".into());
    let raw_kind = value("--kind")?;
    let kind: EventKind = serde_json::from_str(&format!("\"{raw_kind}\""))
        .with_context(|| format!("unsupported event kind {raw_kind}"))?;
    let correlation_id = args
        .iter()
        .position(|argument| argument == "--correlation")
        .and_then(|position| args.get(position + 1).cloned());
    let now = Utc::now();
    let event_id = stable_event_id(
        &provider,
        &instance_id,
        &session_id,
        correlation_id.as_deref(),
        &raw_kind,
        None,
    );
    let mut attributes = BTreeMap::new();
    if let Some(project) = flag_value(&args, "--project").and_then(|value| project_name(&value)) {
        attributes.insert("project".into(), Value::String(project));
    }
    Ok(ActivityEvent {
        schema_version: SCHEMA_VERSION.into(),
        event_id,
        provider: provider.clone(),
        adapter_id: format!("generic.{provider}"),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        source_kind: SourceKind::NativeHook,
        instance_id,
        session_id,
        turn_id: None,
        correlation_id,
        sequence: None,
        kind,
        occurred_at: now,
        observed_at: now,
        tool: None,
        attributes,
    })
}

fn map_hook_event(provider: &str, payload: Value) -> Result<ActivityEvent> {
    let raw_kind = string_field(&payload, &["hook_event_name", "event", "type"])
        .context("hook event name missing")?;
    let kind = match raw_kind {
        "SessionStart" | "session_start" => EventKind::SessionStarted,
        "UserPromptSubmit" | "user_prompt_submit" => EventKind::UserPrompted,
        "Notification" | "notification" | "PreCompact" | "pre_compact" | "PostCompact"
        | "post_compact" | "SubagentStart" | "subagent_start" | "SubagentStop"
        | "subagent_stop" => EventKind::ModelWorking,
        "PreToolUse" | "pre_tool_use" => EventKind::ToolStarted,
        "PermissionRequest" | "permission_request" => EventKind::ApprovalRequired,
        "ApprovalResolved"
        | "approval_resolved"
        | "PermissionResponse"
        | "permission_response"
        | "PermissionResult"
        | "permission_result"
        | "PermissionDenied"
        | "permission_denied"
        | "PermissionRejected"
        | "permission_rejected" => EventKind::ApprovalResolved,
        "PostToolUse" | "post_tool_use" => EventKind::ToolFinished,
        "PostToolUseFailure" | "post_tool_use_failure" => EventKind::ToolFailed,
        "Stop" | "stop" => EventKind::RunCompleted,
        "TurnAborted" | "turn_aborted" => EventKind::RunAborted,
        "StopFailure" | "stop_failure" => EventKind::RunFailed,
        "SessionEnd" | "session_end" => EventKind::SessionStopped,
        other => anyhow::bail!("unsupported hook event: {other}"),
    };
    let session_id = string_field(&payload, &["session_id", "sessionId"])
        .context("hook session id missing")?
        .to_owned();
    let correlation_id = string_field(
        &payload,
        &["call_id", "tool_use_id", "correlation_id", "toolUseId"],
    )
    .map(str::to_owned);
    let provider_event_id = string_field(&payload, &["event_id", "id"]);
    let occurred_at = string_field(&payload, &["timestamp", "occurred_at"])
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    let observed_at = Utc::now();
    let default_instance = format!("{provider}-local");
    let instance_id = string_field(&payload, &["instance_id"])
        .unwrap_or(default_instance.as_str())
        .to_owned();
    let kind_name = serde_json::to_string(&kind).unwrap_or_default();
    let event_id = provider_event_id.map(str::to_owned).unwrap_or_else(|| {
        stable_event_id(
            provider,
            &instance_id,
            &session_id,
            correlation_id.as_deref(),
            &kind_name,
            Some(&payload),
        )
    });
    let tool_name = string_field(&payload, &["tool_name", "toolName"]);
    let tool = tool_name.map(|name| ToolDescriptor {
        name: name.to_owned(),
        category: categorize_tool(name).into(),
    });
    let mut attributes = BTreeMap::new();
    let project = string_field(
        &payload,
        &[
            "project_name",
            "project",
            "cwd",
            "workspace",
            "workspace_path",
            "project_path",
        ],
    )
    .and_then(project_name)
    .or_else(current_project_name);
    if let Some(project) = project {
        attributes.insert("project".into(), Value::String(project));
    }
    if kind == EventKind::ApprovalResolved {
        let decision = if matches!(
            raw_kind,
            "PermissionDenied" | "permission_denied" | "PermissionRejected" | "permission_rejected"
        ) {
            Some(Value::String("rejected".into()))
        } else {
            scalar_field(
                &payload,
                &["approval_decision", "decision", "outcome", "approved"],
            )
            .cloned()
        };
        if let Some(decision) = decision {
            attributes.insert("approval_decision".into(), decision);
        }
    }
    Ok(ActivityEvent {
        schema_version: SCHEMA_VERSION.into(),
        event_id,
        provider: provider.to_owned(),
        adapter_id: format!("builtin.{provider}"),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        source_kind: SourceKind::NativeHook,
        instance_id,
        session_id,
        turn_id: string_field(&payload, &["turn_id", "turnId"]).map(str::to_owned),
        correlation_id,
        sequence: payload.get("sequence").and_then(Value::as_u64),
        kind,
        occurred_at,
        observed_at,
        tool,
        // Only the final path component is retained; raw paths and payload content are discarded.
        attributes,
    })
}

fn inject_event_name(payload: &mut Value, event: Option<String>) -> Result<()> {
    if string_field(payload, &["hook_event_name", "event", "type"]).is_some() {
        return Ok(());
    }
    let Some(event) = event else {
        return Ok(());
    };
    payload
        .as_object_mut()
        .context("hook payload must be a JSON object")?
        .insert("hook_event_name".into(), Value::String(event));
    Ok(())
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|argument| argument == name)
        .and_then(|position| args.get(position + 1).cloned())
}

fn string_field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| value.get(name).and_then(Value::as_str))
}

fn scalar_field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| {
        value
            .get(name)
            .filter(|field| field.is_string() || field.is_boolean() || field.is_number())
    })
}

fn project_name(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches(['/', '\\']);
    let leaf = trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())?
        .trim();
    if leaf.is_empty() {
        return None;
    }
    Some(leaf.chars().take(80).collect())
}

fn current_project_name() -> Option<String> {
    std::env::current_dir()
        .ok()
        .and_then(|path| path.to_str().and_then(project_name))
}

fn categorize_tool(name: &str) -> &'static str {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("shell") || normalized.contains("exec") {
        "execution"
    } else if normalized.contains("write") || normalized.contains("edit") {
        "filesystem"
    } else if normalized.contains("web") || normalized.contains("http") {
        "network"
    } else {
        "other"
    }
}

fn stable_event_id(
    provider: &str,
    instance: &str,
    session: &str,
    correlation: Option<&str>,
    kind: &str,
    fallback: Option<&Value>,
) -> String {
    if let Some(correlation) = correlation {
        return format!("{provider}:{instance}:{session}:{correlation}:{kind}");
    }
    let mut hasher = Sha256::new();
    hasher.update(provider.as_bytes());
    hasher.update(instance.as_bytes());
    hasher.update(session.as_bytes());
    hasher.update(kind.as_bytes());
    if let Some(fallback) = fallback {
        hasher.update(fallback.to_string().as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{provider}:{instance}:{session}:{}", &digest[..20])
}

fn try_start_background() {
    let executable = background_executable();
    let _ = Command::new(executable)
        .arg("--background")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn background_executable() -> PathBuf {
    if let Some(configured) = std::env::var_os("AGENT_ACTIVITY_APP") {
        return PathBuf::from(configured);
    }
    if let Ok(current) = std::env::current_exe() {
        let executable_name = if cfg!(windows) {
            "agent-activity.exe"
        } else {
            "agent-activity"
        };
        let sibling = current.with_file_name(executable_name);
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("agent-activity")
}

async fn append_spool(event: &ActivityEvent) -> Result<()> {
    let dirs = ProjectDirs::from("work", "Effective Work", "Agent Activity Hub")
        .context("cannot resolve application data directory")?;
    let spool_dir = dirs.data_local_dir().join("spool");
    tokio::fs::create_dir_all(&spool_dir).await?;
    let path = spool_dir.join("hook-events.jsonl");
    rotate_spool(&path).await?;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    let mut payload = serde_json::to_vec(event)?;
    payload.push(b'\n');
    file.write_all(&payload).await?;
    file.flush().await?;
    Ok(())
}

async fn rotate_spool(path: &Path) -> Result<()> {
    if tokio::fs::metadata(path)
        .await
        .map(|metadata| metadata.len() >= MAX_SPOOL_BYTES)
        .unwrap_or(false)
    {
        let rotated = path.with_extension("jsonl.previous");
        let _ = tokio::fs::remove_file(&rotated).await;
        tokio::fs::rename(path, rotated).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_permission_maps_without_sensitive_fields() {
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session-1",
            "call_id": "call-1",
            "tool_name": "shell_command",
            "tool_input": {"command": "secret command"},
            "prompt": "secret prompt"
        });
        let event = map_hook_event("codex", payload).unwrap();
        assert_eq!(event.provider, "codex");
        assert_eq!(event.adapter_id, "builtin.codex");
        assert_eq!(event.instance_id, "codex-local");
        assert_eq!(event.kind, EventKind::ApprovalRequired);
        assert_eq!(event.correlation_id.as_deref(), Some("call-1"));
        let serialized = serde_json::to_string(&event).unwrap();
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn qoder_permission_maps_without_sensitive_fields() {
        let payload = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "qoder-session-1",
            "call_id": "qoder-call-1",
            "cwd": "/tmp/project",
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf /"},
            "prompt": "secret qoder prompt"
        });
        let event = map_hook_event("qoder", payload).unwrap();
        assert_eq!(event.provider, "qoder");
        assert_eq!(event.adapter_id, "builtin.qoder");
        assert_eq!(event.instance_id, "qoder-local");
        assert_eq!(event.kind, EventKind::ApprovalRequired);
        assert_eq!(event.correlation_id.as_deref(), Some("qoder-call-1"));
        assert_eq!(
            event.attributes.get("project"),
            Some(&serde_json::json!("project"))
        );
        let serialized = serde_json::to_string(&event).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("rm -rf"));
        assert!(!serialized.contains("/tmp"));
    }

    #[test]
    fn project_name_supports_unix_and_windows_paths() {
        assert_eq!(project_name("/work/gtjaqh/"), Some("gtjaqh".into()));
        assert_eq!(
            project_name(r"C:\work\agent_collaboration_control"),
            Some("agent_collaboration_control".into())
        );
    }

    #[test]
    fn qoder_stop_event_maps_to_run_completed() {
        let payload = serde_json::json!({
            "hook_event_name": "Stop",
            "session_id": "qoder-session-2"
        });
        let event = map_hook_event("qoder", payload).unwrap();
        assert_eq!(event.kind, EventKind::RunCompleted);
        assert_eq!(event.provider, "qoder");
    }

    #[test]
    fn qoder_stop_failure_maps_to_run_failed() {
        let payload = serde_json::json!({
            "hook_event_name": "StopFailure",
            "session_id": "qoder-session-3"
        });
        let event = map_hook_event("qoder", payload).unwrap();
        assert_eq!(event.kind, EventKind::RunFailed);
        assert_eq!(event.provider, "qoder");
    }

    #[test]
    fn codex_turn_aborted_maps_to_run_aborted() {
        let payload = serde_json::json!({
            "hook_event_name": "turn_aborted",
            "session_id": "codex-session-aborted",
            "turn_id": "turn-1"
        });
        let event = map_hook_event("codex", payload).unwrap();
        assert_eq!(event.kind, EventKind::RunAborted);
        assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
    }

    #[test]
    fn explicit_permission_rejection_keeps_only_the_decision() {
        let payload = serde_json::json!({
            "hook_event_name": "PermissionResponse",
            "session_id": "claude-session-1",
            "tool_use_id": "call-1",
            "decision": "rejected",
            "reason": "sensitive rejection reason"
        });
        let event = map_hook_event("claude", payload).unwrap();
        assert_eq!(event.kind, EventKind::ApprovalResolved);
        assert_eq!(
            event.attributes.get("approval_decision"),
            Some(&serde_json::json!("rejected"))
        );
        assert!(!serde_json::to_string(&event)
            .unwrap()
            .contains("sensitive rejection reason"));
    }

    #[test]
    fn explicit_permission_denied_event_is_normalized() {
        let payload = serde_json::json!({
            "hook_event_name": "PermissionDenied",
            "session_id": "qoder-session-4",
            "call_id": "call-2"
        });
        let event = map_hook_event("qoder", payload).unwrap();
        assert_eq!(event.kind, EventKind::ApprovalResolved);
        assert_eq!(
            event.attributes.get("approval_decision"),
            Some(&serde_json::json!("rejected"))
        );
    }

    #[test]
    fn hook_event_without_session_is_rejected() {
        let payload = serde_json::json!({
            "hook_event_name": "Stop"
        });
        assert!(map_hook_event("codex", payload).is_err());
    }

    #[test]
    fn explicit_event_can_be_injected_when_stdin_omits_it() {
        let mut payload = serde_json::json!({
            "session_id": "codex-session",
            "tool_name": "shell_command"
        });
        inject_event_name(&mut payload, Some("PermissionRequest".into())).unwrap();
        let event = map_hook_event("codex", payload).unwrap();
        assert_eq!(event.kind, EventKind::ApprovalRequired);
    }

    #[test]
    fn qoder_supplemental_lifecycle_event_maps_to_working() {
        let payload = serde_json::json!({
            "hook_event_name": "PreCompact",
            "session_id": "qoder-compact",
            "instance_id": "qoder-local"
        });

        let event = map_hook_event("qoder", payload).unwrap();

        assert_eq!(event.kind, EventKind::ModelWorking);
        assert_eq!(event.provider, "qoder");
        assert_eq!(event.session_id, "qoder-compact");
    }

    #[test]
    fn rejected_ipc_response_is_not_delivery_success() {
        assert!(ensure_accepted(Response::error("store_error:disk full")).is_err());
        assert!(ensure_accepted(Response::ok("state_changed")).is_ok());
    }
}
