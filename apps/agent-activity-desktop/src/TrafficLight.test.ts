import { describe, expect, it } from "vitest";
import { floatingLightSize, TRAFFIC_LIGHT_COLOR_ORDER } from "./TrafficLight";

describe("traffic light order", () => {
  it("renders green, yellow, then red", () => {
    expect(TRAFFIC_LIGHT_COLOR_ORDER).toEqual(["green", "amber", "red"]);
  });
});

describe("floatingLightSize", () => {
  it("keeps the existing size for a single agent row", () => {
    expect(floatingLightSize("vertical", false, 32)).toMatchObject({ width: 112, height: 222 });
    expect(floatingLightSize("horizontal", true, 32)).toMatchObject({ width: 184, height: 374 });
  });

  it("adds the wrapped agent rows to collapsed and expanded windows", () => {
    expect(floatingLightSize("horizontal", false, 72)).toMatchObject({ width: 184, height: 170 });
    expect(floatingLightSize("vertical", true, 72)).toMatchObject({ width: 112, height: 506 });
  });
});
