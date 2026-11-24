import { describe, expect, it } from "vitest";
import { relativeLuminance, slopeRamp, verticesThroughLayer } from "./viewport-style";

describe("slope ramp", () => {
  it("keeps Andromeda's low end darker than the accent", () => {
    const ramp = slopeRamp("#b9b3ff", "#06070b");
    expect(ramp.accent).toBe("#b9b3ff");
    const gap = relativeLuminance(ramp.accent) - relativeLuminance(ramp.dim);
    expect(gap).toBeGreaterThan(0.25);
  });

  it("keeps a light theme's accent brighter than the dim end", () => {
    const ramp = slopeRamp("#1d5fd6", "#eef0f4");
    expect(relativeLuminance(ramp.accent)).toBeLessThan(relativeLuminance(ramp.dim));
    expect(ramp.dim).not.toBe(ramp.accent);
  });
});

describe("layer draw range", () => {
  it("draws only the vertices up to the selected layer", () => {
    const counts = [10, 40, 90, 200];
    expect(verticesThroughLayer(counts, 0)).toBe(10);
    expect(verticesThroughLayer(counts, 2)).toBe(90);
    expect(verticesThroughLayer(counts, 99)).toBe(200);
    expect(verticesThroughLayer([], 0)).toBe(0);
  });
});
