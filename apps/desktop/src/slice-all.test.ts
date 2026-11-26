import { describe, expect, it } from "vitest";
import { batchFraction, batchMessage, platesToSlice } from "./slice-all";

describe("platesToSlice", () => {
  const plates = [
    { index: 0, name: "plate 1", models: 2, sliced: true },
    { index: 1, name: "plate 2", models: 0, sliced: false },
    { index: 2, name: "plate 3", models: 1, sliced: false },
  ];

  it("skips an empty plate and keeps the ones with parts", () => {
    expect(platesToSlice(plates, false).map((plate) => plate.index)).toEqual([0, 2]);
  });

  it("skips a plate that already has a slice when only the rest are wanted", () => {
    expect(platesToSlice(plates, true).map((plate) => plate.name)).toEqual(["plate 3"]);
  });
});

describe("batch progress", () => {
  it("spreads one plate's fraction across the batch", () => {
    expect(batchFraction(0, 2, 0)).toBe(0);
    expect(batchFraction(0, 2, 1)).toBeCloseTo(0.5);
    expect(batchFraction(1, 2, 0.5)).toBeCloseTo(0.75);
    expect(batchFraction(1, 2, 1)).toBe(1);
  });

  it("names the plate in front of the engine's own sentence", () => {
    expect(batchMessage(1, 3, "plate 2", "Slicing layer 4")).toBe(
      "Plate 2 of 3: plate 2 — Slicing layer 4",
    );
  });
});
