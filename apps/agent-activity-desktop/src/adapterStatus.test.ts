import { describe, expect, it } from "vitest";
import {
  adapterCanConfigure,
  adapterNeedsRepair,
  adapterStateLabelKey,
  adapterStateTone,
} from "./adapterStatus";
import type { AdapterStatus } from "./types";

const base: AdapterStatus = {
  provider: "qoder",
  state: "not_installed",
  agent_detected: true,
  config_exists: true,
  config_path: "/tmp/settings.json",
  helper_available: true,
  helper_path: "/tmp/agent-activity-hook",
  installed_events: 0,
  total_events: 9,
  missing_events: [],
  legacy_entries: 0,
  error: null,
};

describe("adapter status presentation", () => {
  it("routes partial and legacy installations through repair", () => {
    expect(adapterNeedsRepair("partial")).toBe(true);
    expect(adapterNeedsRepair("legacy")).toBe(true);
    expect(adapterNeedsRepair("not_installed")).toBe(false);
  });

  it("only allows configuration when the bundled helper is usable", () => {
    expect(adapterCanConfigure(base)).toBe(true);
    expect(adapterCanConfigure({ ...base, helper_available: false })).toBe(false);
    expect(adapterCanConfigure({ ...base, state: "error" })).toBe(false);
  });

  it("keeps state labels and tones distinct", () => {
    expect(adapterStateLabelKey("installed")).toBe("adapter.state.installed");
    expect(adapterStateTone("installed")).toBe("green");
    expect(adapterStateTone("legacy")).toBe("amber");
    expect(adapterStateTone("error")).toBe("red");
  });
});
