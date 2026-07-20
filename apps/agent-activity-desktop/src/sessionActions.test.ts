import { describe, expect, it } from "vitest";
import { isDismissibleSessionStatus } from "./sessionActions";

describe("session dismissal availability", () => {
  it("allows every visible session to be dismissed", () => {
    expect(isDismissibleSessionStatus("error")).toBe(true);
    expect(isDismissibleSessionStatus("idle")).toBe(true);
    expect(isDismissibleSessionStatus("offline")).toBe(true);
    expect(isDismissibleSessionStatus("working")).toBe(true);
    expect(isDismissibleSessionStatus("waiting_approval")).toBe(true);
    expect(isDismissibleSessionStatus("complete")).toBe(true);
    expect(isDismissibleSessionStatus("sleeping")).toBe(false);
  });
});
