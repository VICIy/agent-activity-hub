import type { SessionStatus } from "./types";
import type { TranslationKey } from "./i18n";

export const statusLabelKey: Record<SessionStatus, TranslationKey> = {
  offline: "status.offline",
  idle: "status.idle",
  working: "status.working",
  waiting_approval: "status.waiting_approval",
  complete: "status.complete",
  error: "status.error",
  sleeping: "status.sleeping",
};

export const floatingStatusLabelKey: Record<SessionStatus, TranslationKey> = {
  offline: "light.status.offline",
  idle: "light.status.idle",
  working: "light.status.working",
  waiting_approval: "light.status.waiting_approval",
  complete: "light.status.complete",
  error: "light.status.error",
  sleeping: "light.status.sleeping",
};

export const statusTone: Record<SessionStatus, "red" | "amber" | "green" | "muted"> = {
  error: "red",
  waiting_approval: "amber",
  working: "green",
  complete: "green",
  idle: "muted",
  offline: "muted",
  sleeping: "muted",
};
