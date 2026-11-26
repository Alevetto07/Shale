import { describe, expect, it } from "vitest";
import { cutPlanePosition, worldBox } from "./coords";

describe("worldBox", () => {
  it("follows a translation", () => {
    const box = worldBox([0, 0, 0, 10, 0, 0, 0, 4, 8], {
      translation: [2, 0, 1],
      rotation_deg: [0, 0, 0],
      scale: [1, 1, 1],
    });
    expect(box.min).toEqual([2, 0, 1]);
    expect(box.max).toEqual([12, 4, 9]);
  });
});

describe("cutPlanePosition", () => {
  const box = { min: [0, 0, 0] as [number, number, number], max: [10, 10, 20] as [number, number, number] };

  it("puts a zero offset through the middle", () => {
    expect(cutPlanePosition(box, 2, 0)).toBe(10);
  });

  it("keeps the plane inside the part", () => {
    expect(cutPlanePosition(box, 2, 100)).toBeCloseTo(19.8);
    expect(cutPlanePosition(box, 0, -3)).toBeCloseTo(2);
  });
});
