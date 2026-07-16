import { describe, expect, it } from "vitest";
import {
  DEFAULT_LED_MAPPING,
  resolveStatusLedEffect,
  type LedMapping,
} from "./ledEffects";

describe("Tauri traffic light effects", () => {
  it("keeps working solid but makes completion blink in 500 ms phases", () => {
    const working = resolveStatusLedEffect("working", DEFAULT_LED_MAPPING);
    const complete = resolveStatusLedEffect("complete", DEFAULT_LED_MAPPING);

    expect(working).toEqual({ bits: [true, false, false], blink: false, period: 500 });
    expect(complete).toEqual({ bits: [true, false, false], blink: true, period: 500 });
    expect(complete.period * 2).toBe(1000);
  });

  it("turns every lamp off after completion reaches idle", () => {
    expect(resolveStatusLedEffect("idle", DEFAULT_LED_MAPPING).bits).toEqual([false, false, false]);
  });

  it("uses the saved mapping instead of hard-coded status colors", () => {
    const custom: LedMapping = {
      effects: {
        complete: { effect: { pattern: "blink", mask: "011", period: 740 } },
      },
    };

    expect(resolveStatusLedEffect("complete", custom)).toEqual({
      bits: [false, true, true],
      blink: true,
      period: 740,
    });
  });
});
