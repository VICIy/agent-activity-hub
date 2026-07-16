import type { AdapterInstallState, AdapterStatus } from "./types";
import type { TranslationKey } from "./i18n";

const stateKeys: Record<AdapterInstallState, TranslationKey> = {
  installed: "adapter.state.installed",
  partial: "adapter.state.partial",
  legacy: "adapter.state.legacy",
  not_installed: "adapter.state.notInstalled",
  not_detected: "adapter.state.notDetected",
  error: "adapter.state.error",
};

export function adapterStateLabelKey(state: AdapterInstallState): TranslationKey {
  return stateKeys[state];
}

export function adapterNeedsRepair(state: AdapterInstallState): boolean {
  return state === "installed" || state === "partial" || state === "legacy";
}

export function adapterCanConfigure(status: AdapterStatus): boolean {
  return status.helper_available && status.state !== "error";
}

export function adapterStateTone(state: AdapterInstallState): "green" | "amber" | "red" | "muted" {
  if (state === "installed") return "green";
  if (state === "partial" || state === "legacy") return "amber";
  if (state === "error") return "red";
  return "muted";
}
