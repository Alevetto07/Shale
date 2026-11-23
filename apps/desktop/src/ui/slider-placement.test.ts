import { describe, expect, it } from "vitest";
import {
  formatSliderLabel,
  sliderValueLabelPercent,
  sliderValueOnFill,
} from "./slider-placement";

describe("sliderValueLabelPercent", () => {
  const positions = [0, 25, 50, 75, 100];

  it("keeps the value label clear of the thumb at min, mid, and max", () => {
    for (const thumb of positions) {
      const label = sliderValueLabelPercent(thumb);
      expect(Math.abs(label - thumb)).toBeGreaterThanOrEqual(12);
      expect(label).toBeGreaterThanOrEqual(6);
      expect(label).toBeLessThanOrEqual(94);
    }
  });

  it("formats value and unit as one label", () => {
    expect(formatSliderLabel(45, 0, "°")).toBe("45°");
    expect(formatSliderLabel(0.2, 2, "mm")).toBe("0.20 mm");
  });
});

describe("sliderValueOnFill", () => {
  it("marks labels that sit on the filled range", () => {
    expect(sliderValueOnFill(50, 30)).toBe(true);
    expect(sliderValueOnFill(50, 75)).toBe(false);
  });
});