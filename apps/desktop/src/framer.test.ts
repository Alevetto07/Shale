import { describe, expect, it } from "vitest";

import { canFrameBed, modelsToFrame, shouldFrameModels } from "./framer";

describe("viewport framing", () => {
  it("frames the model after import even when the bed size was still zero", () => {
    expect(shouldFrameModels([{ positions: [0, 0, 0, 10, 0, 0, 0, 10, 0] }])).toBe(true);
    expect(canFrameBed(0, 0)).toBe(false);
    expect(canFrameBed(256, 256)).toBe(true);
  });

  it("prefers the selected part when several models are on the plate", () => {
    const parts = modelsToFrame([
      { positions: [1, 2, 3], selected: false },
      { positions: [4, 5, 6], selected: true },
    ]);
    expect(parts).toHaveLength(1);
    expect(parts[0].positions[0]).toBe(4);
  });
});