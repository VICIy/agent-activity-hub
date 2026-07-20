export type FloatingLightOrientation = "vertical" | "horizontal";

export const FLOATING_LIGHT_ORIENTATION_EVENT = "light://orientation";
export const DEFAULT_FLOATING_LIGHT_ORIENTATION: FloatingLightOrientation = "horizontal";
const ORIENTATION_STORAGE_KEY = "agent-activity.orientation";

export function isFloatingLightOrientation(value: unknown): value is FloatingLightOrientation {
  return value === "vertical" || value === "horizontal";
}

export function readFloatingLightOrientation(): FloatingLightOrientation {
  try {
    const stored = window.localStorage.getItem(ORIENTATION_STORAGE_KEY);
    if (isFloatingLightOrientation(stored)) return stored;
  } catch {
    /* ignore */
  }
  return DEFAULT_FLOATING_LIGHT_ORIENTATION;
}

export function writeFloatingLightOrientation(orientation: FloatingLightOrientation): void {
  try {
    window.localStorage.setItem(ORIENTATION_STORAGE_KEY, orientation);
  } catch {
    /* ignore */
  }
}
