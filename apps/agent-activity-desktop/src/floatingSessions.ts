import type { SessionState, SessionStatus } from "./types";

const STATUS_PRIORITY: Record<SessionStatus, number> = {
  error: 500,
  waiting_approval: 400,
  complete: 350,
  working: 300,
  idle: 100,
  offline: 0,
  sleeping: 0,
};

const ACTIVE_STATUSES = new Set<SessionStatus>([
  "error",
  "waiting_approval",
  "complete",
  "working",
]);

const PANEL_STATUSES = new Set<SessionStatus>([
  ...ACTIVE_STATUSES,
  "idle",
]);

export interface ActiveProviderSummary {
  provider: string;
  status: SessionStatus;
  sessionCount: number;
}

export function statusPriority(status: SessionStatus): number {
  return STATUS_PRIORITY[status];
}

export function summarizeActiveProviders(sessions: readonly SessionState[]): ActiveProviderSummary[] {
  const summaries = new Map<string, ActiveProviderSummary>();
  for (const session of sessions) {
    if (!ACTIVE_STATUSES.has(session.status)) continue;
    const current = summaries.get(session.key.provider);
    if (!current) {
      summaries.set(session.key.provider, {
        provider: session.key.provider,
        status: session.status,
        sessionCount: 1,
      });
      continue;
    }
    current.sessionCount += 1;
    if (statusPriority(session.status) > statusPriority(current.status)) {
      current.status = session.status;
    }
  }
  return [...summaries.values()].sort(
    (left, right) =>
      statusPriority(right.status) - statusPriority(left.status)
      || left.provider.localeCompare(right.provider),
  );
}

export function floatingPanelSessions(
  sessions: readonly SessionState[],
  limit = 50,
): SessionState[] {
  return sessions
    .filter((session) => PANEL_STATUSES.has(session.status))
    .sort(
      (left, right) =>
        statusPriority(right.status) - statusPriority(left.status)
        || Date.parse(right.entered_at) - Date.parse(left.entered_at)
        || left.key.provider.localeCompare(right.key.provider),
    )
    .slice(0, limit);
}
