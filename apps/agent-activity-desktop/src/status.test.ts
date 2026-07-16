import { describe, expect, it } from "vitest";
import { statusLabelKey, statusTone } from "./status";

describe("status presentation", () => {
  it("keeps approvals visually distinct from failures", () => {
    expect(statusTone.waiting_approval).toBe("amber");
    expect(statusTone.error).toBe("red");
    expect(statusLabelKey.waiting_approval).toBe("status.waiting_approval");
  });
});
