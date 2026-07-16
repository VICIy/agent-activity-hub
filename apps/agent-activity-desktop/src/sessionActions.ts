import { invoke, isTauri } from "@tauri-apps/api/core";
import type { SessionKey, SessionStatus } from "./types";

export function isDismissibleSessionStatus(status: SessionStatus): boolean {
  return status === "error" || status === "idle" || status === "offline";
}

export async function dismissSession(key: SessionKey): Promise<boolean> {
  if (!isTauri()) return false;
  return invoke<boolean>("dismiss_session", { key });
}
