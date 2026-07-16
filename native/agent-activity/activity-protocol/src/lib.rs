use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    NativeHook,
    OfficialApi,
    SessionLog,
    ProcessHeuristic,
}

impl SourceKind {
    pub const fn priority(self) -> u16 {
        match self {
            Self::NativeHook => 400,
            Self::OfficialApi => 300,
            Self::SessionLog => 200,
            Self::ProcessHeuristic => 100,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    #[serde(rename = "adapter.connected")]
    AdapterConnected,
    #[serde(rename = "adapter.disconnected")]
    AdapterDisconnected,
    #[serde(rename = "session.started")]
    SessionStarted,
    #[serde(rename = "session.stopped")]
    SessionStopped,
    #[serde(rename = "user.prompted")]
    UserPrompted,
    #[serde(rename = "model.working")]
    ModelWorking,
    #[serde(rename = "tool.started")]
    ToolStarted,
    #[serde(rename = "tool.finished")]
    ToolFinished,
    #[serde(rename = "tool.failed")]
    ToolFailed,
    #[serde(rename = "approval.required")]
    ApprovalRequired,
    #[serde(rename = "approval.resolved")]
    ApprovalResolved,
    #[serde(rename = "run.completed")]
    RunCompleted,
    #[serde(rename = "run.aborted")]
    RunAborted,
    #[serde(rename = "run.failed")]
    RunFailed,
    #[serde(rename = "heartbeat")]
    Heartbeat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescriptor {
    pub name: String,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityEvent {
    pub schema_version: String,
    pub event_id: String,
    pub provider: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub source_kind: SourceKind,
    pub instance_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    pub kind: EventKind,
    pub occurred_at: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolDescriptor>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("unsupported schema version: {0}")]
    UnsupportedVersion(String),
    #[error("required field is empty: {0}")]
    EmptyField(&'static str),
    #[error("provider contains unsupported characters")]
    InvalidProvider,
    #[error("field exceeds maximum length: {0} (max {1})")]
    FieldTooLong(&'static str, usize),
    #[error("attribute is not a scalar: {0}")]
    NonScalarAttribute(String),
    #[error("heuristic sources can only report adapter presence")]
    UntrustedSemanticEvent,
}

impl ActivityEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedVersion(
                self.schema_version.clone(),
            ));
        }
        for (name, value) in [
            ("event_id", self.event_id.as_str()),
            ("provider", self.provider.as_str()),
            ("adapter_id", self.adapter_id.as_str()),
            ("adapter_version", self.adapter_version.as_str()),
            ("instance_id", self.instance_id.as_str()),
            ("session_id", self.session_id.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ValidationError::EmptyField(name));
            }
        }
        let mut provider_chars = self.provider.chars();
        let valid_first = provider_chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let valid_rest = provider_chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
        if !valid_first || !valid_rest {
            return Err(ValidationError::InvalidProvider);
        }
        if self.event_id.len() > 512 {
            return Err(ValidationError::FieldTooLong("event_id", 512));
        }
        if let Some(tool) = &self.tool {
            if tool.name.len() > 128 {
                return Err(ValidationError::FieldTooLong("tool.name", 128));
            }
            if tool.category.len() > 64 {
                return Err(ValidationError::FieldTooLong("tool.category", 64));
            }
        }
        if self
            .attributes
            .iter()
            .any(|(_, value)| value.is_array() || value.is_object())
        {
            let key = self
                .attributes
                .iter()
                .find(|(_, value)| value.is_array() || value.is_object())
                .map(|(key, _)| key.clone())
                .unwrap_or_default();
            return Err(ValidationError::NonScalarAttribute(key));
        }
        if self.source_kind == SourceKind::ProcessHeuristic
            && !matches!(
                self.kind,
                EventKind::AdapterConnected | EventKind::AdapterDisconnected | EventKind::Heartbeat
            )
        {
            return Err(ValidationError::UntrustedSemanticEvent);
        }
        Ok(())
    }

    pub fn session_key(&self) -> SessionKey {
        SessionKey {
            provider: self.provider.clone(),
            instance_id: self.instance_id.clone(),
            session_id: self.session_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey {
    pub provider: String,
    pub instance_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Offline,
    Idle,
    Working,
    WaitingApproval,
    Complete,
    Error,
    Sleeping,
}

impl SessionStatus {
    pub const fn priority(self) -> u16 {
        match self {
            Self::Error => 500,
            Self::WaitingApproval => 400,
            // Completion is a short notification lease. Keep it visible over
            // concurrent work, then let the arbiter return to that work.
            Self::Complete => 350,
            Self::Working => 300,
            Self::Idle => 100,
            Self::Offline | Self::Sleeping => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterventionCommand {
    pub schema_version: String,
    pub command_id: String,
    pub kind: CommandKind,
    pub provider: String,
    pub instance_id: String,
    pub session_id: String,
    pub correlation_id: Option<String>,
    pub expected_revision: u64,
    pub issued_by: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandKind {
    #[serde(rename = "approval.approve")]
    ApprovalApprove,
    #[serde(rename = "approval.reject")]
    ApprovalReject,
    #[serde(rename = "run.pause")]
    RunPause,
    #[serde(rename = "run.resume")]
    RunResume,
    #[serde(rename = "run.cancel")]
    RunCancel,
    #[serde(rename = "session.focus")]
    SessionFocus,
    #[serde(rename = "message.send")]
    MessageSend,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_status_priority_matches_the_traffic_light_contract() {
        let ordered = [
            SessionStatus::Error,
            SessionStatus::WaitingApproval,
            SessionStatus::Complete,
            SessionStatus::Working,
            SessionStatus::Idle,
        ];

        assert!(ordered
            .windows(2)
            .all(|pair| pair[0].priority() > pair[1].priority()));
    }

    #[test]
    fn run_aborted_uses_the_standard_protocol_name() {
        assert_eq!(
            serde_json::to_string(&EventKind::RunAborted).unwrap(),
            r#""run.aborted""#
        );
        assert_eq!(
            serde_json::from_str::<EventKind>(r#""run.aborted""#).unwrap(),
            EventKind::RunAborted
        );
    }

    #[test]
    fn heuristic_approval_is_rejected() {
        let event = ActivityEvent {
            schema_version: SCHEMA_VERSION.into(),
            event_id: "evt-1".into(),
            provider: "codex".into(),
            adapter_id: "builtin.codex".into(),
            adapter_version: "0.1.0".into(),
            source_kind: SourceKind::ProcessHeuristic,
            instance_id: "local".into(),
            session_id: "s1".into(),
            turn_id: None,
            correlation_id: Some("c1".into()),
            sequence: None,
            kind: EventKind::ApprovalRequired,
            occurred_at: Utc::now(),
            observed_at: Utc::now(),
            tool: None,
            attributes: BTreeMap::new(),
        };
        assert_eq!(
            event.validate(),
            Err(ValidationError::UntrustedSemanticEvent)
        );
    }

    #[test]
    fn provider_must_start_with_alphanumeric_character() {
        let mut event = heuristic_event();
        event.provider = ".codex".into();
        event.kind = EventKind::Heartbeat;
        assert_eq!(event.validate(), Err(ValidationError::InvalidProvider));
    }

    #[test]
    fn heuristic_presence_event_is_accepted() {
        let mut event = heuristic_event();
        event.kind = EventKind::Heartbeat;
        assert_eq!(event.validate(), Ok(()));
    }

    fn heuristic_event() -> ActivityEvent {
        ActivityEvent {
            schema_version: SCHEMA_VERSION.into(),
            event_id: "evt-1".into(),
            provider: "codex".into(),
            adapter_id: "builtin.codex".into(),
            adapter_version: "0.1.0".into(),
            source_kind: SourceKind::ProcessHeuristic,
            instance_id: "local".into(),
            session_id: "s1".into(),
            turn_id: None,
            correlation_id: Some("c1".into()),
            sequence: None,
            kind: EventKind::ApprovalRequired,
            occurred_at: Utc::now(),
            observed_at: Utc::now(),
            tool: None,
            attributes: BTreeMap::new(),
        }
    }
}
