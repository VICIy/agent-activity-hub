import type { SessionStatus } from "./types";

export type LedEffect =
  | { leds: string }
  | { effect: { pattern: string; mask: string; period: number } };

export interface LedMapping {
  effects: Record<string, LedEffect>;
}

export interface LedSettings {
  mapping: LedMapping;
  brightness: number;
  statuses: string[];
}

export interface LedDisplaySettings {
  mapping: LedMapping;
  brightness: number;
}

export interface ResolvedLedEffect {
  bits: [boolean, boolean, boolean];
  blink: boolean;
  period: number;
}

export const MIN_LED_PERIOD = 20;
export const MAX_LED_PERIOD = 10_000;
export const DEFAULT_LED_PERIOD = 500;
export const MIN_LED_BRIGHTNESS = 10;
export const MAX_LED_BRIGHTNESS = 100;
export const DEFAULT_LED_BRIGHTNESS = 80;

export const FALLBACK_STATUSES: SessionStatus[] = [
  "offline",
  "idle",
  "working",
  "waiting_approval",
  "complete",
  "error",
  "sleeping",
];

export const DEFAULT_LED_MAPPING: LedMapping = {
  effects: {
    offline: { leds: "000" },
    idle: { leds: "000" },
    working: { leds: "100" },
    waiting_approval: { effect: { pattern: "blink", mask: "010", period: DEFAULT_LED_PERIOD } },
    complete: { effect: { pattern: "blink", mask: "100", period: DEFAULT_LED_PERIOD } },
    error: { effect: { pattern: "blink", mask: "001", period: DEFAULT_LED_PERIOD } },
    sleeping: { leds: "000" },
  },
};

export function normalizeLedMask(mask: string): [boolean, boolean, boolean] {
  const padded = (mask ?? "").padEnd(3, "0").slice(0, 3);
  return [padded[0] === "1", padded[1] === "1", padded[2] === "1"];
}

function bitsToMask(bits: readonly boolean[]): string {
  return bits.map((bit) => (bit ? "1" : "0")).join("");
}

export function clampLedPeriod(period: number): number {
  if (!Number.isFinite(period)) return DEFAULT_LED_PERIOD;
  return Math.max(MIN_LED_PERIOD, Math.min(MAX_LED_PERIOD, Math.round(period)));
}

export function clampLedBrightness(brightness: number): number {
  if (!Number.isFinite(brightness)) return DEFAULT_LED_BRIGHTNESS;
  return Math.max(MIN_LED_BRIGHTNESS, Math.min(MAX_LED_BRIGHTNESS, Math.round(brightness)));
}

export function readLedEffect(effect: LedEffect | undefined): ResolvedLedEffect {
  if (!effect) {
    return { bits: [false, false, false], blink: false, period: DEFAULT_LED_PERIOD };
  }
  if ("leds" in effect) {
    return { bits: normalizeLedMask(effect.leds), blink: false, period: DEFAULT_LED_PERIOD };
  }
  return {
    bits: normalizeLedMask(effect.effect.mask),
    blink: effect.effect.pattern === "blink",
    period: clampLedPeriod(effect.effect.period),
  };
}

export function resolveStatusLedEffect(status: SessionStatus, mapping: LedMapping): ResolvedLedEffect {
  return readLedEffect(mapping.effects[status] ?? DEFAULT_LED_MAPPING.effects[status]);
}

export function buildEffect(bits: readonly boolean[], blink: boolean, period: number): LedEffect {
  const mask = bitsToMask(bits);
  if (!blink) return { leds: mask };
  return { effect: { pattern: "blink", mask, period: clampLedPeriod(period) } };
}
