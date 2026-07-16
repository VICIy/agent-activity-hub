use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use activity_protocol::{
    ActivityEvent, EventKind, SessionKey, SessionStatus, SourceKind, ValidationError,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const COMPLETE_LEASE_SECONDS: i64 = 5;
const MAX_DEDUPE_KEYS: usize = 20_000;
const MAX_INFLIGHT_TOOLS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
struct InflightTool {
    turn_id: Option<String>,
    tool_name: Option<String>,
    started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionState {
    pub key: SessionKey,
    #[serde(default)]
    pub project: Option<String>,
    pub status: SessionStatus,
    pub entered_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
    #[serde(default)]
    pub last_sequence: Option<u64>,
    pub source_event_id: String,
    pub active_correlation_ids: BTreeSet<String>,
    #[serde(default)]
    approval_rejected: bool,
    // Runtime-only context for providers whose approval hook omits the tool call ID.
    #[serde(skip)]
    inflight_tools: BTreeMap<String, InflightTool>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub reason: String,
    pub revision: u64,
    pub source_kind: SourceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GlobalState {
    pub status: SessionStatus,
    pub provider: Option<String>,
    pub instance_id: Option<String>,
    pub session_id: Option<String>,
    pub since: DateTime<Utc>,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateSnapshot {
    pub global: GlobalState,
    pub sessions: Vec<SessionState>,
    pub deduplicated_events: u64,
    pub accepted_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEngine {
    #[serde(with = "sessions_as_vec")]
    sessions: HashMap<SessionKey, SessionState>,
    #[serde(default)]
    dedupe: HashMap<String, u16>,
    #[serde(default)]
    dedupe_order: VecDeque<String>,
    #[serde(default)]
    global_revision: u64,
    #[serde(default)]
    deduplicated_events: u64,
    #[serde(default)]
    accepted_events: u64,
}

mod sessions_as_vec {
    use std::collections::HashMap;

    use activity_protocol::SessionKey;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::SessionState;

    pub fn serialize<S>(
        sessions: &HashMap<SessionKey, SessionState>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let values: Vec<&SessionState> = sessions.values().collect();
        values.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<SessionKey, SessionState>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = serde_json::Value::deserialize(deserializer)?;
        let values: Vec<SessionState> = match raw {
            serde_json::Value::Array(_) => {
                serde_json::from_value(raw).map_err(serde::de::Error::custom)?
            }
            _ => Vec::new(),
        };
        Ok(values
            .into_iter()
            .map(|state| (state.key.clone(), state))
            .collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    StateChanged,
    AcceptedNoChange,
    Duplicate,
    DuplicateSourceUpgraded,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    InvalidEvent(#[from] ValidationError),
}

impl Default for ActivityEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityEngine {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            dedupe: HashMap::new(),
            dedupe_order: VecDeque::new(),
            global_revision: 0,
            deduplicated_events: 0,
            accepted_events: 0,
        }
    }

    pub fn apply(&mut self, event: ActivityEvent) -> Result<ApplyOutcome, EngineError> {
        event.validate()?;
        self.rebuild_legacy_dedupe_order();
        let priority = event.source_kind.priority();
        let dedupe_keys = dedupe_keys(&event);
        let previous_priority = dedupe_keys
            .iter()
            .filter_map(|key| self.dedupe.get(key).copied())
            .max();
        if let Some(previous_priority) = previous_priority {
            self.deduplicated_events += 1;
            if priority > previous_priority {
                for key in dedupe_keys {
                    self.remember_dedupe_key(key, priority);
                }
                if let Some(state) = self.sessions.get_mut(&event.session_key()) {
                    if state.source_event_id == event.event_id
                        || state.source_kind.priority() < priority
                    {
                        state.source_kind = event.source_kind;
                    }
                }
                return Ok(ApplyOutcome::DuplicateSourceUpgraded);
            }
            return Ok(ApplyOutcome::Duplicate);
        }
        for key in dedupe_keys {
            self.remember_dedupe_key(key, priority);
        }

        self.accepted_events += 1;
        let key = event.session_key();
        let incoming_project = event
            .attributes
            .get("project")
            .and_then(serde_json::Value::as_str)
            .filter(|project| !project.trim().is_empty())
            .map(str::to_owned);
        if event.kind == EventKind::RunAborted && !self.sessions.contains_key(&key) {
            // Session-log replay must not repopulate the UI with historical sessions.
            return Ok(ApplyOutcome::AcceptedNoChange);
        }
        let state = self
            .sessions
            .entry(key.clone())
            .or_insert_with(|| SessionState {
                key,
                project: incoming_project.clone(),
                status: SessionStatus::Offline,
                entered_at: event.observed_at,
                last_event_at: event.occurred_at,
                last_sequence: None,
                source_event_id: event.event_id.clone(),
                active_correlation_ids: BTreeSet::new(),
                approval_rejected: false,
                inflight_tools: BTreeMap::new(),
                lease_expires_at: None,
                reason: "first event observed".into(),
                revision: 0,
                source_kind: event.source_kind,
            });

        if is_stale(state, &event) {
            return Ok(ApplyOutcome::AcceptedNoChange);
        }

        let previous_status = state.status;
        let previous_project = state.project.clone();
        let previous_correlations = state.active_correlation_ids.clone();
        let previous_approval_rejected = state.approval_rejected;
        let previous_lease = state.lease_expires_at;
        let previous_reason = state.reason.clone();
        if let Some(project) = incoming_project {
            state.project = Some(project);
        }
        reduce_session(state, &event);
        state.last_event_at = event.occurred_at;
        if event.sequence.is_some() {
            state.last_sequence = event.sequence;
        }

        let state_changed = state.status != previous_status
            || state.project != previous_project
            || state.active_correlation_ids != previous_correlations
            || state.approval_rejected != previous_approval_rejected
            || state.lease_expires_at != previous_lease
            || state.reason != previous_reason;
        if state_changed {
            state.source_event_id = event.event_id;
            state.source_kind = event.source_kind;
            state.revision += 1;
            self.global_revision += 1;
            Ok(ApplyOutcome::StateChanged)
        } else {
            Ok(ApplyOutcome::AcceptedNoChange)
        }
    }

    pub fn expire_leases(&mut self, now: DateTime<Utc>) -> bool {
        let mut changed = false;
        for state in self.sessions.values_mut() {
            let expired = state
                .lease_expires_at
                .is_some_and(|expires_at| expires_at <= now);
            if expired && state.status == SessionStatus::Complete {
                state.status = SessionStatus::Idle;
                state.entered_at = now;
                state.lease_expires_at = None;
                state.reason = "display lease expired".into();
                state.revision += 1;
                changed = true;
            }
        }
        if changed {
            self.global_revision += 1;
        }
        changed
    }

    pub fn dismiss_error(&mut self, key: &SessionKey, now: DateTime<Utc>) -> bool {
        let Some(state) = self.sessions.get_mut(key) else {
            return false;
        };
        if state.status != SessionStatus::Error {
            return false;
        }
        state.status = SessionStatus::Idle;
        state.entered_at = now;
        state.lease_expires_at = None;
        state.reason = "error dismissed by user".into();
        state.revision += 1;
        self.global_revision += 1;
        true
    }

    pub fn dismiss_session(&mut self, key: &SessionKey, now: DateTime<Utc>) -> bool {
        match self.sessions.get(key).map(|state| state.status) {
            Some(SessionStatus::Error) => self.dismiss_error(key, now),
            Some(SessionStatus::Idle | SessionStatus::Offline) => {
                self.sessions.remove(key);
                // Keep event dedupe history. A genuinely new provider event has a new
                // identity and recreates the session with its latest project and status.
                self.global_revision += 1;
                true
            }
            _ => false,
        }
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> StateSnapshot {
        let mut sessions: Vec<_> = self.sessions.values().cloned().collect();
        sessions.sort_by(|left, right| {
            right
                .status
                .priority()
                .cmp(&left.status.priority())
                .then_with(|| right.entered_at.cmp(&left.entered_at))
                .then_with(|| left.key.provider.cmp(&right.key.provider))
        });
        // Offline/sleeping sessions remain available for per-session cleanup,
        // but they must not make the global indicator look disconnected when
        // there is no active work left.
        let global_session = sessions
            .iter()
            .find(|session| {
                !matches!(
                    session.status,
                    SessionStatus::Offline | SessionStatus::Sleeping
                )
            });
        let global = global_session.map_or(
            GlobalState {
                // An empty or all-offline session list is idle from the user's
                // perspective. Offline remains a per-session diagnostic state.
                status: SessionStatus::Idle,
                provider: None,
                instance_id: None,
                session_id: None,
                since: now,
                revision: self.global_revision,
            },
            |selected| GlobalState {
                status: selected.status,
                provider: Some(selected.key.provider.clone()),
                instance_id: Some(selected.key.instance_id.clone()),
                session_id: Some(selected.key.session_id.clone()),
                since: selected.entered_at,
                revision: self.global_revision,
            },
        );
        StateSnapshot {
            global,
            sessions,
            deduplicated_events: self.deduplicated_events,
            accepted_events: self.accepted_events,
        }
    }

    pub fn restore_verified(mut snapshot: Self) -> Self {
        snapshot.rebuild_legacy_dedupe_order();
        let mut changed = false;
        for state in snapshot.sessions.values_mut() {
            if state.status == SessionStatus::Complete && state.lease_expires_at.is_none() {
                state.status = SessionStatus::Idle;
                state.reason = "stale terminal status cleared during restart".into();
                state.revision += 1;
                changed = true;
            }
        }
        if changed {
            snapshot.global_revision += 1;
        }
        snapshot
    }

    fn rebuild_legacy_dedupe_order(&mut self) {
        if !self.dedupe_order.is_empty() || self.dedupe.is_empty() {
            return;
        }
        let mut entries: Vec<_> = std::mem::take(&mut self.dedupe).into_iter().collect();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        if entries.len() > MAX_DEDUPE_KEYS {
            entries.drain(..entries.len() - MAX_DEDUPE_KEYS);
        }
        for (key, priority) in entries {
            self.dedupe_order.push_back(key.clone());
            self.dedupe.insert(key, priority);
        }
    }

    fn remember_dedupe_key(&mut self, key: String, priority: u16) {
        if !self.dedupe.contains_key(&key) {
            self.dedupe_order.push_back(key.clone());
        }
        self.dedupe.insert(key, priority);
        while self.dedupe_order.len() > MAX_DEDUPE_KEYS {
            if let Some(expired) = self.dedupe_order.pop_front() {
                self.dedupe.remove(&expired);
            }
        }
    }
}

fn dedupe_keys(event: &ActivityEvent) -> Vec<String> {
    let mut keys = vec![format!(
        "event:{}:{}:{}",
        event.provider, event.instance_id, event.event_id
    )];
    if let Some(correlation_id) = &event.correlation_id {
        keys.push(format!(
            "correlation:{}:{}:{}:{}:{:?}",
            event.provider, event.instance_id, event.session_id, correlation_id, event.kind
        ));
    } else if let Some(sequence) = event.sequence {
        keys.push(format!(
            "sequence:{}:{}:{}:{}:{:?}",
            event.provider, event.instance_id, event.session_id, sequence, event.kind
        ));
    }
    keys
}

fn is_stale(state: &SessionState, event: &ActivityEvent) -> bool {
    if event.kind == EventKind::RunAborted
        && state.status == SessionStatus::WaitingApproval
        && event.occurred_at >= state.entered_at
    {
        // A session-log abort can be replayed after newer native tool events on
        // application restart. It still resolves the approval that preceded it.
        return false;
    }
    if let (Some(previous), Some(next)) = (state.last_sequence, event.sequence) {
        return next <= previous;
    }
    event.occurred_at < state.last_event_at
}

fn reduce_session(state: &mut SessionState, event: &ActivityEvent) {
    use EventKind::*;
    if !matches!(event.kind, SessionStopped | Heartbeat) {
        state.approval_rejected = false;
    }
    match event.kind {
        ApprovalRequired => {
            if state.status == SessionStatus::WaitingApproval {
                // Anchor replayed aborts to the newest approval request in this session.
                state.entered_at = event.observed_at;
            }
            let correlation = event
                .correlation_id
                .clone()
                .or_else(|| infer_inflight_correlation(state, event))
                .unwrap_or_else(|| format!("event:{}", event.event_id));
            state.active_correlation_ids.insert(correlation);
            transition(
                state,
                SessionStatus::WaitingApproval,
                event,
                "approval required",
                None,
            );
        }
        ApprovalResolved => {
            resolve_correlation(state, event, false);
            if approval_was_rejected(event) && state.active_correlation_ids.is_empty() {
                state.approval_rejected = true;
                transition(state, SessionStatus::Idle, event, "approval rejected", None);
            } else if state.active_correlation_ids.is_empty() {
                transition(
                    state,
                    SessionStatus::Working,
                    event,
                    "approval resolved",
                    None,
                );
            }
        }
        ToolFinished => {
            resolve_correlation(state, event, false);
            finish_inflight_tool(state, event);
            if state.active_correlation_ids.is_empty() {
                transition(state, SessionStatus::Working, event, "tool finished", None);
            }
        }
        ToolFailed => {
            resolve_correlation(state, event, false);
            finish_inflight_tool(state, event);
            transition(state, SessionStatus::Error, event, "tool failed", None);
        }
        ModelWorking => {
            if state.status == SessionStatus::WaitingApproval {
                if event.source_kind.priority() < state.source_kind.priority() {
                    return;
                }
                state.active_correlation_ids.clear();
            }
            transition(state, SessionStatus::Working, event, "model working", None);
        }
        ToolStarted => {
            resolve_correlation(state, event, true);
            remember_inflight_tool(state, event);
            if state.active_correlation_ids.is_empty() {
                transition(state, SessionStatus::Working, event, "tool started", None);
            }
        }
        UserPrompted => {
            // A new prompt belongs to a new run. Some providers do not emit an
            // approval-resolution event after the user rejects the previous request.
            state.active_correlation_ids.clear();
            state.inflight_tools.clear();
            transition(state, SessionStatus::Working, event, "user prompted", None);
        }
        RunCompleted => {
            let approval_was_pending = state.status == SessionStatus::WaitingApproval;
            state.active_correlation_ids.clear();
            state.inflight_tools.clear();
            if approval_was_pending {
                // Providers do not consistently expose an explicit denial hook. A Stop
                // received while permission is still pending is the observable "No" path.
                state.approval_rejected = true;
                transition(state, SessionStatus::Idle, event, "approval rejected", None);
            } else {
                transition(
                    state,
                    SessionStatus::Complete,
                    event,
                    "run completed",
                    Some(Duration::seconds(COMPLETE_LEASE_SECONDS)),
                );
            }
        }
        RunAborted => {
            let approval_was_pending = state.status == SessionStatus::WaitingApproval;
            state.active_correlation_ids.clear();
            state.inflight_tools.clear();
            state.approval_rejected = approval_was_pending;
            transition(
                state,
                SessionStatus::Idle,
                event,
                if approval_was_pending {
                    "approval rejected"
                } else {
                    "run aborted"
                },
                None,
            );
        }
        RunFailed => {
            state.active_correlation_ids.clear();
            state.inflight_tools.clear();
            transition(state, SessionStatus::Error, event, "run failed", None);
        }
        SessionStarted => {
            state.active_correlation_ids.clear();
            state.inflight_tools.clear();
            transition(state, SessionStatus::Idle, event, "session started", None);
        }
        SessionStopped => {
            state.active_correlation_ids.clear();
            state.inflight_tools.clear();
            // Claude emits SessionEnd immediately after Stop/StopFailure. Errors are
            // persistent; completion keeps its short display lease before becoming idle.
            if state.status == SessionStatus::Error {
                state.reason = "session ended after failure".into();
            } else if state.status == SessionStatus::Complete && state.lease_expires_at.is_some() {
                state.reason = "session ended during terminal display lease".into();
            } else if state.status == SessionStatus::Idle && state.approval_rejected {
                state.reason = "session ended after approval rejection".into();
            } else {
                transition(
                    state,
                    SessionStatus::Offline,
                    event,
                    "session unavailable",
                    None,
                );
            }
        }
        AdapterDisconnected => {
            state.active_correlation_ids.clear();
            state.inflight_tools.clear();
            transition(
                state,
                SessionStatus::Offline,
                event,
                "adapter disconnected",
                None,
            );
        }
        AdapterConnected => {
            if state.status == SessionStatus::Offline {
                transition(state, SessionStatus::Idle, event, "adapter connected", None);
            }
        }
        Heartbeat => {}
    }
}

fn approval_was_rejected(event: &ActivityEvent) -> bool {
    const DECISION_KEYS: [&str; 4] = ["approval_decision", "decision", "outcome", "approved"];
    DECISION_KEYS.iter().any(|key| {
        event.attributes.get(*key).is_some_and(|value| match value {
            serde_json::Value::Bool(approved) => !approved,
            serde_json::Value::String(decision) => matches!(
                decision.trim().to_ascii_lowercase().as_str(),
                "no" | "deny" | "denied" | "reject" | "rejected" | "decline" | "declined"
            ),
            _ => false,
        })
    })
}

fn resolve_correlation(state: &mut SessionState, event: &ActivityEvent, only_if_present: bool) {
    if let Some(correlation_id) = &event.correlation_id {
        state.active_correlation_ids.remove(correlation_id);
    } else if !only_if_present {
        state.active_correlation_ids.clear();
    }
}

fn remember_inflight_tool(state: &mut SessionState, event: &ActivityEvent) {
    let Some(correlation_id) = event.correlation_id.clone() else {
        return;
    };
    state.inflight_tools.insert(
        correlation_id,
        InflightTool {
            turn_id: event.turn_id.clone(),
            tool_name: event.tool.as_ref().map(|tool| tool.name.clone()),
            started_at: event.occurred_at,
        },
    );
    while state.inflight_tools.len() > MAX_INFLIGHT_TOOLS {
        let Some(oldest) = state
            .inflight_tools
            .iter()
            .min_by_key(|(_, tool)| tool.started_at)
            .map(|(correlation_id, _)| correlation_id.clone())
        else {
            break;
        };
        state.inflight_tools.remove(&oldest);
    }
}

fn infer_inflight_correlation(state: &SessionState, event: &ActivityEvent) -> Option<String> {
    let turn_id = event.turn_id.as_ref()?;
    let tool_name = event.tool.as_ref().map(|tool| tool.name.as_str())?;

    state
        .inflight_tools
        .iter()
        .filter(|(_, tool)| {
            tool.turn_id.as_ref() == Some(turn_id) && tool.tool_name.as_deref() == Some(tool_name)
        })
        .max_by(|(left_id, left), (right_id, right)| {
            left.started_at
                .cmp(&right.started_at)
                .then_with(|| left_id.cmp(right_id))
        })
        .map(|(correlation_id, _)| correlation_id.clone())
}

fn finish_inflight_tool(state: &mut SessionState, event: &ActivityEvent) {
    if let Some(correlation_id) = &event.correlation_id {
        state.inflight_tools.remove(correlation_id);
    }
}

fn transition(
    state: &mut SessionState,
    status: SessionStatus,
    event: &ActivityEvent,
    reason: &str,
    lease: Option<Duration>,
) {
    if state.status != status {
        state.entered_at = event.observed_at;
    }
    state.status = status;
    state.reason = reason.into();
    state.lease_expires_at = lease.map(|duration| event.observed_at + duration);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use activity_protocol::{ActivityEvent, EventKind, SourceKind, ToolDescriptor, SCHEMA_VERSION};
    use chrono::{TimeZone, Utc};

    use super::*;

    fn event(session: &str, id: &str, kind: EventKind, correlation: Option<&str>) -> ActivityEvent {
        let timestamp = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        ActivityEvent {
            schema_version: SCHEMA_VERSION.into(),
            event_id: id.into(),
            provider: "codex".into(),
            adapter_id: "builtin.codex".into(),
            adapter_version: "0.1.0".into(),
            source_kind: SourceKind::NativeHook,
            instance_id: "local".into(),
            session_id: session.into(),
            turn_id: None,
            correlation_id: correlation.map(str::to_owned),
            sequence: None,
            kind,
            occurred_at: timestamp,
            observed_at: timestamp,
            tool: None,
            attributes: BTreeMap::new(),
        }
    }

    fn provider_event(
        provider: &str,
        session: &str,
        id: &str,
        kind: EventKind,
        correlation: Option<&str>,
        offset_seconds: i64,
    ) -> ActivityEvent {
        let mut value = event(session, id, kind, correlation);
        value.provider = provider.into();
        value.adapter_id = format!("builtin.{provider}");
        value.instance_id = format!("{provider}-local");
        value.occurred_at += Duration::seconds(offset_seconds);
        value.observed_at += Duration::seconds(offset_seconds);
        value
    }

    fn tool_event(
        session: &str,
        id: &str,
        kind: EventKind,
        correlation: Option<&str>,
        turn_id: &str,
        tool_name: &str,
        offset_seconds: i64,
    ) -> ActivityEvent {
        let mut value = event(session, id, kind, correlation);
        value.turn_id = Some(turn_id.into());
        value.tool = Some(ToolDescriptor {
            name: tool_name.into(),
            category: "other".into(),
        });
        value.occurred_at += Duration::seconds(offset_seconds);
        value.observed_at += Duration::seconds(offset_seconds);
        value
    }

    #[test]
    fn another_session_cannot_clear_waiting() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "waiting",
                "a1",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        engine
            .apply(event("working", "b1", EventKind::ModelWorking, None))
            .unwrap();
        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.global.status, SessionStatus::WaitingApproval);
        assert_eq!(snapshot.global.session_id.as_deref(), Some("waiting"));
    }

    #[test]
    fn unrelated_tool_does_not_clear_waiting() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "a1",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        engine
            .apply(event("s1", "a2", EventKind::ToolFinished, Some("call-2")))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::WaitingApproval
        );
    }

    #[test]
    fn uncorrelated_approval_binds_to_matching_inflight_tool() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(tool_event(
                "s1",
                "started",
                EventKind::ToolStarted,
                Some("call-1"),
                "turn-1",
                "Bash",
                0,
            ))
            .unwrap();
        engine
            .apply(tool_event(
                "s1",
                "approval",
                EventKind::ApprovalRequired,
                None,
                "turn-1",
                "Bash",
                1,
            ))
            .unwrap();

        let waiting = engine.snapshot(Utc::now());
        assert_eq!(waiting.global.status, SessionStatus::WaitingApproval);
        assert_eq!(
            waiting.sessions[0].active_correlation_ids,
            BTreeSet::from(["call-1".into()])
        );

        engine
            .apply(tool_event(
                "s1",
                "finished",
                EventKind::ToolFinished,
                Some("call-1"),
                "turn-1",
                "Bash",
                2,
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
    }

    #[test]
    fn uncorrelated_approval_does_not_bind_to_same_tool_in_another_turn() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(tool_event(
                "s1",
                "bash-started",
                EventKind::ToolStarted,
                Some("bash-call"),
                "turn-1",
                "Bash",
                0,
            ))
            .unwrap();
        engine
            .apply(tool_event(
                "s1",
                "other-started",
                EventKind::ToolStarted,
                Some("other-call"),
                "turn-2",
                "Bash",
                1,
            ))
            .unwrap();
        engine
            .apply(tool_event(
                "s1",
                "approval",
                EventKind::ApprovalRequired,
                None,
                "turn-1",
                "Bash",
                2,
            ))
            .unwrap();
        engine
            .apply(tool_event(
                "s1",
                "other-finished",
                EventKind::ToolFinished,
                Some("other-call"),
                "turn-2",
                "Bash",
                3,
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::WaitingApproval
        );

        engine
            .apply(tool_event(
                "s1",
                "bash-finished",
                EventKind::ToolFinished,
                Some("bash-call"),
                "turn-1",
                "Bash",
                4,
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
    }

    #[test]
    fn matching_resolution_clears_waiting() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "a1",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        engine
            .apply(event(
                "s1",
                "a2",
                EventKind::ApprovalResolved,
                Some("call-1"),
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
    }

    #[test]
    fn explicit_rejected_resolution_returns_only_that_session_to_idle() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "rejected",
                "reject-required",
                EventKind::ApprovalRequired,
                Some("reject-call"),
            ))
            .unwrap();
        engine
            .apply(event(
                "still-waiting",
                "wait-required",
                EventKind::ApprovalRequired,
                Some("wait-call"),
            ))
            .unwrap();

        let mut rejected = event(
            "rejected",
            "reject-resolved",
            EventKind::ApprovalResolved,
            Some("reject-call"),
        );
        rejected
            .attributes
            .insert("decision".into(), serde_json::json!("no"));
        engine.apply(rejected).unwrap();

        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.global.status, SessionStatus::WaitingApproval);
        assert_eq!(snapshot.global.session_id.as_deref(), Some("still-waiting"));
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .find(|session| session.key.session_id == "rejected")
                .unwrap()
                .status,
            SessionStatus::Idle
        );
    }

    #[test]
    fn aborted_approval_returns_only_that_session_to_idle() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "rejected",
                "reject-required",
                EventKind::ApprovalRequired,
                Some("reject-call"),
            ))
            .unwrap();
        engine
            .apply(event(
                "still-waiting",
                "wait-required",
                EventKind::ApprovalRequired,
                Some("wait-call"),
            ))
            .unwrap();
        engine
            .apply(event(
                "rejected",
                "turn-aborted",
                EventKind::RunAborted,
                None,
            ))
            .unwrap();

        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.global.status, SessionStatus::WaitingApproval);
        assert_eq!(snapshot.global.session_id.as_deref(), Some("still-waiting"));
        let rejected = snapshot
            .sessions
            .iter()
            .find(|session| session.key.session_id == "rejected")
            .unwrap();
        assert_eq!(rejected.status, SessionStatus::Idle);
        assert!(rejected.active_correlation_ids.is_empty());
        assert_eq!(rejected.reason, "approval rejected");
    }

    #[test]
    fn new_prompt_clears_an_unresolved_approval_for_that_session() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "approval",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        engine
            .apply(event("s1", "next-prompt", EventKind::UserPrompted, None))
            .unwrap();

        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.global.status, SessionStatus::Working);
        assert!(snapshot.sessions[0].active_correlation_ids.is_empty());
    }

    #[test]
    fn historical_abort_does_not_recreate_a_missing_session() {
        let mut engine = ActivityEngine::new();
        assert_eq!(
            engine
                .apply(event(
                    "historical",
                    "old-abort",
                    EventKind::RunAborted,
                    None,
                ))
                .unwrap(),
            ApplyOutcome::AcceptedNoChange
        );
        assert!(engine.snapshot(Utc::now()).sessions.is_empty());
    }

    #[test]
    fn replayed_abort_clears_waiting_after_newer_unrelated_tool_events() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "approval",
                EventKind::ApprovalRequired,
                Some("approval-call"),
            ))
            .unwrap();
        engine
            .apply(tool_event(
                "s1",
                "unrelated-finished",
                EventKind::ToolFinished,
                Some("another-call"),
                "turn-2",
                "Bash",
                5,
            ))
            .unwrap();
        let mut replayed_abort = event("s1", "abort", EventKind::RunAborted, None);
        replayed_abort.occurred_at += Duration::seconds(1);
        replayed_abort.observed_at += Duration::seconds(6);

        engine.apply(replayed_abort).unwrap();

        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Idle
        );
    }

    #[test]
    fn old_abort_cannot_clear_a_newer_approval_in_the_same_session() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "approval-1",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        let mut approval_2 = event(
            "s1",
            "approval-2",
            EventKind::ApprovalRequired,
            Some("call-2"),
        );
        approval_2.occurred_at += Duration::seconds(10);
        approval_2.observed_at += Duration::seconds(10);
        engine.apply(approval_2).unwrap();

        let mut old_abort = event("s1", "old-abort", EventKind::RunAborted, None);
        old_abort.occurred_at += Duration::seconds(5);
        old_abort.observed_at += Duration::seconds(11);
        engine.apply(old_abort).unwrap();

        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.global.status, SessionStatus::WaitingApproval);
        assert_eq!(snapshot.sessions[0].active_correlation_ids.len(), 2);
    }

    #[test]
    fn claude_and_qoder_rejections_end_at_idle_without_affecting_each_other() {
        let mut engine = ActivityEngine::new();
        for (provider, offset) in [("claude", 0), ("qoder", 1)] {
            engine
                .apply(provider_event(
                    provider,
                    &format!("{provider}-session"),
                    &format!("{provider}-approval"),
                    EventKind::ApprovalRequired,
                    Some(&format!("{provider}-call")),
                    offset,
                ))
                .unwrap();
        }

        engine
            .apply(provider_event(
                "qoder",
                "qoder-session",
                "qoder-stop-after-no",
                EventKind::RunCompleted,
                None,
                2,
            ))
            .unwrap();
        let after_qoder_no = engine.snapshot(Utc::now());
        assert_eq!(
            after_qoder_no.global.session_id.as_deref(),
            Some("claude-session")
        );
        assert_eq!(after_qoder_no.global.status, SessionStatus::WaitingApproval);
        assert_eq!(
            after_qoder_no
                .sessions
                .iter()
                .find(|session| session.key.provider == "qoder")
                .unwrap()
                .status,
            SessionStatus::Idle
        );

        engine
            .apply(provider_event(
                "claude",
                "claude-session",
                "claude-stop-after-no",
                EventKind::RunCompleted,
                None,
                3,
            ))
            .unwrap();
        assert!(engine
            .snapshot(Utc::now())
            .sessions
            .iter()
            .all(|session| session.status == SessionStatus::Idle));
    }

    #[test]
    fn stop_while_waiting_is_rejection_not_completion() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "approval",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        engine
            .apply(event("s1", "stop", EventKind::RunCompleted, None))
            .unwrap();

        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.global.status, SessionStatus::Idle);
        assert_eq!(snapshot.sessions[0].lease_expires_at, None);
        assert_eq!(snapshot.sessions[0].reason, "approval rejected");
    }

    #[test]
    fn session_end_after_rejection_does_not_override_idle() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "approval",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        engine
            .apply(event("s1", "stop", EventKind::RunCompleted, None))
            .unwrap();
        engine
            .apply(event("s1", "session-end", EventKind::SessionStopped, None))
            .unwrap();

        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.global.status, SessionStatus::Idle);
        assert_eq!(
            snapshot.sessions[0].reason,
            "session ended after approval rejection"
        );
    }

    #[test]
    fn duplicate_event_does_not_advance_revision() {
        let mut engine = ActivityEngine::new();
        let first = event("s1", "a1", EventKind::ModelWorking, None);
        assert_eq!(
            engine.apply(first.clone()).unwrap(),
            ApplyOutcome::StateChanged
        );
        let revision = engine.snapshot(Utc::now()).global.revision;
        assert_eq!(engine.apply(first).unwrap(), ApplyOutcome::Duplicate);
        assert_eq!(engine.snapshot(Utc::now()).global.revision, revision);
    }

    #[test]
    fn heartbeat_refresh_does_not_trigger_output_revision() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event("s1", "a1", EventKind::SessionStarted, None))
            .unwrap();
        let revision = engine.snapshot(Utc::now()).global.revision;
        assert_eq!(
            engine
                .apply(event("s1", "a2", EventKind::Heartbeat, None))
                .unwrap(),
            ApplyOutcome::AcceptedNoChange
        );
        assert_eq!(engine.snapshot(Utc::now()).global.revision, revision);
    }

    #[test]
    fn all_pending_correlations_must_be_resolved() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "a1",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        engine
            .apply(event(
                "s1",
                "a2",
                EventKind::ApprovalRequired,
                Some("call-2"),
            ))
            .unwrap();
        engine
            .apply(event(
                "s1",
                "a3",
                EventKind::ApprovalResolved,
                Some("call-1"),
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::WaitingApproval
        );
        engine
            .apply(event(
                "s1",
                "a4",
                EventKind::ApprovalResolved,
                Some("call-2"),
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
    }

    #[test]
    fn dedupe_is_isolated_by_instance() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "shared-session",
                "a1",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        let mut second = event(
            "shared-session",
            "a2",
            EventKind::ApprovalRequired,
            Some("call-1"),
        );
        second.instance_id = "remote".into();
        assert_eq!(engine.apply(second).unwrap(), ApplyOutcome::StateChanged);
        assert_eq!(engine.snapshot(Utc::now()).sessions.len(), 2);
    }

    #[test]
    fn lower_confidence_working_does_not_clear_native_waiting() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "a1",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        let mut lower_confidence = event("s1", "a2", EventKind::ModelWorking, None);
        lower_confidence.source_kind = SourceKind::SessionLog;
        assert_eq!(
            engine.apply(lower_confidence).unwrap(),
            ApplyOutcome::AcceptedNoChange
        );
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::WaitingApproval
        );
    }

    #[test]
    fn older_timestamp_does_not_roll_state_back() {
        let mut engine = ActivityEngine::new();
        let mut current = event("s1", "a1", EventKind::ModelWorking, None);
        current.occurred_at += Duration::seconds(10);
        current.observed_at += Duration::seconds(10);
        engine.apply(current).unwrap();
        assert_eq!(
            engine
                .apply(event("s1", "a2", EventKind::RunCompleted, None))
                .unwrap(),
            ApplyOutcome::AcceptedNoChange
        );
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
    }

    #[test]
    fn older_sequence_does_not_roll_state_back() {
        let mut engine = ActivityEngine::new();
        let mut current = event("s1", "a1", EventKind::ModelWorking, None);
        current.sequence = Some(2);
        engine.apply(current).unwrap();
        let mut stale = event("s1", "a2", EventKind::RunCompleted, None);
        stale.sequence = Some(1);
        assert_eq!(engine.apply(stale).unwrap(), ApplyOutcome::AcceptedNoChange);
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
    }

    #[test]
    fn only_completion_expires_to_idle() {
        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();

        let mut complete = ActivityEngine::new();
        complete
            .apply(event("complete", "a1", EventKind::RunCompleted, None))
            .unwrap();
        assert!(!complete.expire_leases(base + Duration::seconds(4)));
        assert_eq!(
            complete.snapshot(base).global.status,
            SessionStatus::Complete
        );
        assert!(complete.expire_leases(base + Duration::seconds(5)));
        assert_eq!(complete.snapshot(base).global.status, SessionStatus::Idle);

        let mut error = ActivityEngine::new();
        error
            .apply(event("error", "b1", EventKind::RunFailed, None))
            .unwrap();
        assert_eq!(error.snapshot(base).sessions[0].lease_expires_at, None);
        assert!(!error.expire_leases(base + Duration::days(30)));
        assert_eq!(error.snapshot(base).global.status, SessionStatus::Error);
    }

    #[test]
    fn dismisses_only_the_selected_error_session() {
        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        let mut engine = ActivityEngine::new();
        engine
            .apply(provider_event(
                "codex",
                "error-1",
                "failure-1",
                EventKind::RunFailed,
                None,
                0,
            ))
            .unwrap();
        engine
            .apply(provider_event(
                "claude",
                "error-2",
                "failure-2",
                EventKind::RunFailed,
                None,
                1,
            ))
            .unwrap();
        let first = SessionKey {
            provider: "codex".into(),
            instance_id: "codex-local".into(),
            session_id: "error-1".into(),
        };
        let working = SessionKey {
            provider: "qoder".into(),
            instance_id: "qoder-local".into(),
            session_id: "missing".into(),
        };

        assert!(!engine.dismiss_error(&working, base + Duration::seconds(2)));
        assert!(engine.dismiss_error(&first, base + Duration::seconds(2)));
        let snapshot = engine.snapshot(base + Duration::seconds(2));
        assert_eq!(snapshot.global.status, SessionStatus::Error);
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .find(|session| session.key == first)
                .unwrap()
                .status,
            SessionStatus::Idle
        );
        assert!(!engine.dismiss_error(&first, base + Duration::seconds(3)));
    }

    #[test]
    fn dismissed_error_session_updates_when_work_resumes() {
        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        let mut engine = ActivityEngine::new();
        engine
            .apply(event("error", "failure", EventKind::RunFailed, None))
            .unwrap();
        let key = SessionKey {
            provider: "codex".into(),
            instance_id: "local".into(),
            session_id: "error".into(),
        };

        assert!(engine.dismiss_session(&key, base + Duration::seconds(1)));
        assert_eq!(
            engine.snapshot(base).sessions[0].status,
            SessionStatus::Idle
        );

        let mut resumed = event("error", "resumed", EventKind::UserPrompted, None);
        resumed.occurred_at += Duration::seconds(2);
        resumed.observed_at += Duration::seconds(2);
        engine.apply(resumed).unwrap();
        assert_eq!(
            engine.snapshot(base + Duration::seconds(2)).sessions[0].status,
            SessionStatus::Working
        );
    }

    #[test]
    fn dismissed_idle_session_reappears_with_latest_event_state() {
        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        let mut engine = ActivityEngine::new();
        let mut started = event("idle", "started", EventKind::SessionStarted, None);
        started
            .attributes
            .insert("project".into(), serde_json::json!("old-project"));
        engine.apply(started).unwrap();
        let key = SessionKey {
            provider: "codex".into(),
            instance_id: "local".into(),
            session_id: "idle".into(),
        };

        assert!(engine.dismiss_session(&key, base + Duration::seconds(1)));
        assert!(engine.snapshot(base).sessions.is_empty());
        assert_eq!(engine.snapshot(base).global.status, SessionStatus::Idle);
        assert!(!engine.dismiss_session(&key, base + Duration::seconds(1)));

        let mut resumed = event("idle", "resumed", EventKind::ModelWorking, None);
        resumed.occurred_at += Duration::seconds(2);
        resumed.observed_at += Duration::seconds(2);
        resumed
            .attributes
            .insert("project".into(), serde_json::json!("latest-project"));
        engine.apply(resumed).unwrap();

        let snapshot = engine.snapshot(base + Duration::seconds(2));
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].key, key);
        assert_eq!(snapshot.sessions[0].status, SessionStatus::Working);
        assert_eq!(
            snapshot.sessions[0].project.as_deref(),
            Some("latest-project")
        );
    }

    #[test]
    fn dismissed_offline_session_reappears_when_work_resumes() {
        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        let mut engine = ActivityEngine::new();
        engine
            .apply(event("offline", "started", EventKind::SessionStarted, None))
            .unwrap();
        let mut stopped = event("offline", "stopped", EventKind::SessionStopped, None);
        stopped.occurred_at += Duration::seconds(1);
        stopped.observed_at += Duration::seconds(1);
        engine.apply(stopped).unwrap();
        let key = SessionKey {
            provider: "codex".into(),
            instance_id: "local".into(),
            session_id: "offline".into(),
        };

        assert_eq!(
            engine.snapshot(base).sessions[0].status,
            SessionStatus::Offline
        );
        assert!(engine.dismiss_session(&key, base + Duration::seconds(2)));
        assert!(engine.snapshot(base).sessions.is_empty());

        let mut resumed = event("offline", "resumed", EventKind::ModelWorking, None);
        resumed.occurred_at += Duration::seconds(3);
        resumed.observed_at += Duration::seconds(3);
        engine.apply(resumed).unwrap();
        assert_eq!(
            engine.snapshot(base + Duration::seconds(3)).sessions[0].status,
            SessionStatus::Working
        );
    }

    #[test]
    fn session_stop_preserves_complete_display_lease() {
        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        let mut engine = ActivityEngine::new();
        engine
            .apply(provider_event(
                "claude",
                "claude-session",
                "stop",
                EventKind::RunCompleted,
                None,
                0,
            ))
            .unwrap();
        engine
            .apply(provider_event(
                "claude",
                "claude-session",
                "session-end",
                EventKind::SessionStopped,
                None,
                1,
            ))
            .unwrap();

        let terminal = engine.snapshot(base + Duration::seconds(1));
        assert_eq!(terminal.global.status, SessionStatus::Complete);
        assert_eq!(
            terminal.sessions[0].lease_expires_at,
            Some(base + Duration::seconds(COMPLETE_LEASE_SECONDS))
        );
        assert!(engine.expire_leases(base + Duration::seconds(COMPLETE_LEASE_SECONDS)));
        assert_eq!(engine.snapshot(base).global.status, SessionStatus::Idle);
    }

    #[test]
    fn session_stop_and_restart_preserve_error() {
        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        let mut engine = ActivityEngine::new();
        engine
            .apply(provider_event(
                "claude",
                "claude-session",
                "stop-failure",
                EventKind::RunFailed,
                None,
                0,
            ))
            .unwrap();
        engine
            .apply(provider_event(
                "claude",
                "claude-session",
                "session-end",
                EventKind::SessionStopped,
                None,
                1,
            ))
            .unwrap();

        let terminal = engine.snapshot(base + Duration::seconds(1));
        assert_eq!(terminal.global.status, SessionStatus::Error);
        assert_eq!(terminal.sessions[0].lease_expires_at, None);
        assert!(!engine.expire_leases(base + Duration::days(30)));

        let restored = ActivityEngine::restore_verified(engine);
        assert_eq!(restored.snapshot(base).global.status, SessionStatus::Error);
    }

    #[test]
    fn session_stop_without_terminal_lease_keeps_global_idle() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event("working", "work", EventKind::ModelWorking, None))
            .unwrap();
        engine
            .apply(event(
                "working",
                "session-end",
                EventKind::SessionStopped,
                None,
            ))
            .unwrap();

        let snapshot = engine.snapshot(Utc::now());
        assert_eq!(snapshot.sessions[0].status, SessionStatus::Offline);
        assert_eq!(snapshot.global.status, SessionStatus::Idle);
    }

    #[test]
    fn restart_clears_terminal_status_that_has_no_display_lease() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event("s1", "a1", EventKind::RunCompleted, None))
            .unwrap();
        engine
            .sessions
            .values_mut()
            .next()
            .unwrap()
            .lease_expires_at = None;

        let restored = ActivityEngine::restore_verified(engine);
        let snapshot = restored.snapshot(Utc::now());
        assert_eq!(snapshot.global.status, SessionStatus::Idle);
        assert_eq!(snapshot.sessions[0].lease_expires_at, None);
    }

    #[test]
    fn global_arbiter_handles_many_providers_and_sessions() {
        let mut engine = ActivityEngine::new();
        for (index, (provider, session)) in [
            ("codex", "codex-1"),
            ("codex", "codex-2"),
            ("qoder", "qoder-1"),
            ("qoder", "qoder-2"),
            ("claude", "claude-1"),
            ("claude", "claude-2"),
            ("xxx", "xxx-1"),
        ]
        .into_iter()
        .enumerate()
        {
            engine
                .apply(provider_event(
                    provider,
                    session,
                    &format!("start-{index}"),
                    EventKind::SessionStarted,
                    None,
                    0,
                ))
                .unwrap();
            engine
                .apply(provider_event(
                    provider,
                    session,
                    &format!("work-{index}"),
                    EventKind::ModelWorking,
                    None,
                    1,
                ))
                .unwrap();
        }
        assert_eq!(engine.snapshot(Utc::now()).sessions.len(), 7);
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );

        engine
            .apply(provider_event(
                "codex",
                "codex-2",
                "approval",
                EventKind::ApprovalRequired,
                Some("codex-call"),
                2,
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::WaitingApproval
        );

        engine
            .apply(provider_event(
                "claude",
                "claude-1",
                "failure",
                EventKind::RunFailed,
                None,
                3,
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Error
        );

        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        assert!(!engine.expire_leases(base + Duration::days(30)));
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Error
        );

        engine
            .apply(provider_event(
                "claude",
                "claude-1",
                "failure-recovered",
                EventKind::ModelWorking,
                None,
                18,
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::WaitingApproval
        );

        engine
            .apply(provider_event(
                "codex",
                "codex-2",
                "approval-finished",
                EventKind::ToolFinished,
                Some("codex-call"),
                19,
            ))
            .unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
    }

    #[test]
    fn mixed_agents_projects_and_sessions_fall_back_by_global_priority() {
        let mut engine = ActivityEngine::new();
        let cases = [
            (
                "codex",
                "codex-window-1",
                "codex-error",
                "project-a",
                EventKind::RunFailed,
            ),
            (
                "codex",
                "codex-window-2",
                "codex-waiting",
                "project-b",
                EventKind::ApprovalRequired,
            ),
            (
                "qoder",
                "qoder-window-1",
                "qoder-complete",
                "project-c",
                EventKind::RunCompleted,
            ),
            (
                "claude",
                "claude-window-1",
                "claude-working",
                "project-d",
                EventKind::ModelWorking,
            ),
            (
                "xxx",
                "xxx-window-1",
                "xxx-idle",
                "project-e",
                EventKind::SessionStarted,
            ),
        ];

        for (index, (provider, instance, session, project, kind)) in cases.into_iter().enumerate() {
            let mut value = provider_event(
                provider,
                session,
                &format!("mixed-{index}"),
                kind,
                (kind == EventKind::ApprovalRequired).then_some("approval-call"),
                index as i64,
            );
            value.instance_id = instance.into();
            value
                .attributes
                .insert("project".into(), serde_json::json!(project));
            engine.apply(value).unwrap();
        }

        let initial = engine.snapshot(Utc::now());
        assert_eq!(initial.sessions.len(), 5);
        assert_eq!(initial.global.status, SessionStatus::Error);
        assert_eq!(
            initial
                .sessions
                .iter()
                .filter_map(|session| session.project.as_deref())
                .collect::<BTreeSet<_>>()
                .len(),
            5
        );

        let error_key = SessionKey {
            provider: "codex".into(),
            instance_id: "codex-window-1".into(),
            session_id: "codex-error".into(),
        };
        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        assert!(engine.dismiss_error(&error_key, base + Duration::seconds(10)));
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::WaitingApproval
        );

        let mut rejected = provider_event(
            "codex",
            "codex-waiting",
            "mixed-rejected",
            EventKind::ApprovalResolved,
            Some("approval-call"),
            11,
        );
        rejected.instance_id = "codex-window-2".into();
        rejected
            .attributes
            .insert("approval_decision".into(), serde_json::json!(false));
        engine.apply(rejected).unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Complete
        );

        assert!(engine.expire_leases(base + Duration::seconds(20)));
        let final_snapshot = engine.snapshot(Utc::now());
        assert_eq!(final_snapshot.global.status, SessionStatus::Working);
        assert_eq!(
            final_snapshot.global.session_id.as_deref(),
            Some("claude-working")
        );
        assert_eq!(
            final_snapshot
                .sessions
                .iter()
                .find(|session| session.key.session_id == "codex-waiting")
                .unwrap()
                .status,
            SessionStatus::Idle
        );
    }

    #[test]
    fn completion_notification_temporarily_interrupts_other_work() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(provider_event(
                "codex",
                "still-working",
                "working",
                EventKind::ModelWorking,
                None,
                0,
            ))
            .unwrap();
        engine
            .apply(provider_event(
                "claude",
                "just-finished",
                "complete",
                EventKind::RunCompleted,
                None,
                1,
            ))
            .unwrap();

        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Complete
        );

        let base = Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap();
        assert!(engine.expire_leases(base + Duration::seconds(6)));
        assert_eq!(
            engine.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
    }

    #[test]
    fn dedupe_index_is_bounded() {
        let mut engine = ActivityEngine::new();
        for index in 0..=MAX_DEDUPE_KEYS {
            engine
                .apply(event(
                    "s1",
                    &format!("event-{index}"),
                    EventKind::Heartbeat,
                    None,
                ))
                .unwrap();
        }
        assert_eq!(engine.dedupe.len(), MAX_DEDUPE_KEYS);
        assert_eq!(engine.dedupe_order.len(), MAX_DEDUPE_KEYS);
    }

    #[test]
    fn restart_preserves_waiting_approval() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event(
                "s1",
                "a1",
                EventKind::ApprovalRequired,
                Some("call-1"),
            ))
            .unwrap();
        let restored = ActivityEngine::restore_verified(engine);
        assert_eq!(
            restored.snapshot(Utc::now()).global.status,
            SessionStatus::WaitingApproval
        );
    }

    #[test]
    fn engine_round_trips_through_json() {
        let mut engine = ActivityEngine::new();
        engine
            .apply(event("s1", "a1", EventKind::ModelWorking, None))
            .unwrap();
        let payload = serde_json::to_string(&engine).expect("serialize engine");
        let restored: ActivityEngine = serde_json::from_str(&payload).expect("deserialize engine");
        assert_eq!(
            restored.snapshot(Utc::now()).global.status,
            SessionStatus::Working
        );
        assert_eq!(restored.snapshot(Utc::now()).sessions.len(), 1);
    }

    #[test]
    fn session_project_is_updated_from_redacted_event_metadata() {
        let mut engine = ActivityEngine::new();
        let mut first = event("s1", "a1", EventKind::SessionStarted, None);
        first
            .attributes
            .insert("project".into(), serde_json::Value::String("gtjaqh".into()));
        engine.apply(first).unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).sessions[0].project.as_deref(),
            Some("gtjaqh")
        );

        let mut next = event("s1", "a2", EventKind::ModelWorking, None);
        next.attributes.insert(
            "project".into(),
            serde_json::Value::String("agent_collaboration_control".into()),
        );
        engine.apply(next).unwrap();
        assert_eq!(
            engine.snapshot(Utc::now()).sessions[0].project.as_deref(),
            Some("agent_collaboration_control")
        );
    }

    #[test]
    fn legacy_empty_map_snapshot_deserializes() {
        let legacy = r#"{"sessions":{},"dedupe":{},"global_revision":0,"deduplicated_events":0,"accepted_events":0}"#;
        let engine: ActivityEngine = serde_json::from_str(legacy).expect("legacy snapshot loads");
        assert_eq!(engine.snapshot(Utc::now()).sessions.len(), 0);
    }
}
