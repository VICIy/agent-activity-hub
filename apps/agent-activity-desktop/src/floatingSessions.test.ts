import { describe, expect, it } from "vitest";
import { floatingPanelSessions, summarizeActiveProviders } from "./floatingSessions";
import type { SessionState, SessionStatus } from "./types";

function session(provider: string, id: string, status: SessionStatus, project = "project"): SessionState {
  return {
    key: { provider, instance_id: `${provider}-local`, session_id: id },
    project,
    status,
    entered_at: `2026-07-15T10:00:0${id.length % 10}Z`,
    last_event_at: "2026-07-15T10:00:00Z",
    last_sequence: null,
    source_event_id: `event-${id}`,
    active_correlation_ids: [],
    lease_expires_at: null,
    reason: status,
    revision: 1,
    source_kind: "native_hook",
  };
}

describe("floating light multi-session summaries", () => {
  it("aggregates each provider by its highest-priority active session", () => {
    const summaries = summarizeActiveProviders([
      session("codex", "codex-1", "working"),
      session("codex", "codex-2", "error"),
      session("qoder", "qoder-1", "waiting_approval"),
      session("claude", "claude-1", "complete"),
      session("xxx", "xxx-1", "idle"),
    ]);

    expect(summaries).toEqual([
      { provider: "codex", status: "error", sessionCount: 2 },
      { provider: "qoder", status: "waiting_approval", sessionCount: 1 },
      { provider: "claude", status: "complete", sessionCount: 1 },
    ]);
  });

  it("lists active and idle sessions in arbitration order", () => {
    const sessions = floatingPanelSessions([
      session("xxx", "xxx-1", "idle"),
      session("codex", "codex-1", "working"),
      session("qoder", "qoder-1", "waiting_approval"),
      session("claude", "claude-1", "complete"),
      session("codex", "codex-2", "error"),
      session("old", "old-1", "offline"),
      session("sleep", "sleep-1", "sleeping"),
    ]);

    expect(sessions.map((item) => item.status)).toEqual([
      "error",
      "waiting_approval",
      "complete",
      "working",
      "idle",
    ]);
  });
});
