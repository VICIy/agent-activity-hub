import { describe, expect, it } from "vitest";
import { DEFAULT_FLOATING_LIGHT_ORIENTATION } from "./floatingLightPreferences";

describe("floating light preferences", () => {
  it("defaults new installations to the horizontal layout", () => {
    expect(DEFAULT_FLOATING_LIGHT_ORIENTATION).toBe("horizontal");
  });
});
