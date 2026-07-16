import { describe, expect, it } from "vitest";
import { isDismissibleSessionStatus } from "./sessionActions";

describe("session dismissal availability", () => {
  it("allows errors, idle sessions, and offline sessions to be dismissed", () => {
    expect(isDismissibleSessionStatus("error")).toBe(true);
    expect(isDismissibleSessionStatus("idle")).toBe(true);
    expect(isDismissibleSessionStatus("offline")).toBe(true);
  });

  it("keeps active and terminal notifications visible", () => {
    expect(isDismissibleSessionStatus("working")).toBe(false);
    expect(isDismissibleSessionStatus("waiting_approval")).toBe(false);
    expect(isDismissibleSessionStatus("complete")).toBe(false);
    expect(isDismissibleSessionStatus("sleeping")).toBe(false);
  });
});
