import { describe, expect, it } from "vitest";
import {
  buildEffect,
  clampLedBrightness,
  clampLedPeriod,
  DEFAULT_LED_BRIGHTNESS,
  DEFAULT_LED_PERIOD,
  MAX_LED_BRIGHTNESS,
  MAX_LED_PERIOD,
  MIN_LED_BRIGHTNESS,
  MIN_LED_PERIOD,
} from "./LedSettingsPanel";

describe("LED period settings", () => {
  it("uses 500 ms when the input is not a finite number", () => {
    expect(clampLedPeriod(Number.NaN)).toBe(DEFAULT_LED_PERIOD);
  });

  it("keeps the phase interval within the supported limits", () => {
    expect(clampLedPeriod(1)).toBe(MIN_LED_PERIOD);
    expect(clampLedPeriod(20_000)).toBe(MAX_LED_PERIOD);
    expect(clampLedPeriod(640)).toBe(640);
  });

  it("stores the normalized phase interval in blink effects", () => {
    expect(buildEffect([true, false, false], true, 500)).toEqual({
      effect: { pattern: "blink", mask: "100", period: 500 },
    });
  });
});

describe("LED brightness settings", () => {
  it("defaults invalid values and clamps the supported percentage", () => {
    expect(clampLedBrightness(Number.NaN)).toBe(DEFAULT_LED_BRIGHTNESS);
    expect(clampLedBrightness(0)).toBe(MIN_LED_BRIGHTNESS);
    expect(clampLedBrightness(55)).toBe(55);
    expect(clampLedBrightness(200)).toBe(MAX_LED_BRIGHTNESS);
  });
});
