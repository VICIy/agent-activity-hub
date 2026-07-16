export type SessionStatus =
  | "offline"
  | "idle"
  | "working"
  | "waiting_approval"
  | "complete"
  | "error"
  | "sleeping";

export interface SessionKey {
  provider: string;
  instance_id: string;
  session_id: string;
}

export interface SessionState {
  key: SessionKey;
  project: string | null;
  status: SessionStatus;
  entered_at: string;
  last_event_at: string;
  last_sequence: number | null;
  source_event_id: string;
  active_correlation_ids: string[];
  lease_expires_at: string | null;
  reason: string;
  revision: number;
  source_kind: string;
}

export interface StateSnapshot {
  global: {
    status: SessionStatus;
    provider: string | null;
    instance_id: string | null;
    session_id: string | null;
    since: string;
    revision: number;
  };
  sessions: SessionState[];
  deduplicated_events: number;
  accepted_events: number;
}

export type AdapterProvider = "codex" | "claude" | "qoder";

export type AdapterInstallState =
  | "installed"
  | "partial"
  | "legacy"
  | "not_installed"
  | "not_detected"
  | "error";

export interface AdapterStatus {
  provider: AdapterProvider;
  state: AdapterInstallState;
  agent_detected: boolean;
  config_exists: boolean;
  config_path: string;
  helper_available: boolean;
  helper_path: string | null;
  installed_events: number;
  total_events: number;
  missing_events: string[];
  legacy_entries: number;
  error: string | null;
}

export const emptySnapshot: StateSnapshot = {
  global: {
    status: "idle",
    provider: null,
    instance_id: null,
    session_id: null,
    since: new Date().toISOString(),
    revision: 0,
  },
  sessions: [],
  deduplicated_events: 0,
  accepted_events: 0,
};
