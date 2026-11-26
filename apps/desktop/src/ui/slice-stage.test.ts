import { describe, expect, it } from "vitest";
import { sliceStage } from "./slice-stage";

describe("sliceStage", () => {
  it("names the stages a person can follow", () => {
    expect(sliceStage("Starting")).toBe("Starting");
    expect(sliceStage("Resolving printer profile")).toBe("Resolving the profile");
    expect(sliceStage("Slicing layer 40")).toBe("Slicing layers");
    expect(sliceStage("nonplanar post pass")).toBe("Curving layers");
    expect(sliceStage("")).toBe("Preparing");
    expect(sliceStage("Plate 2 of 3: plate 2 — Slicing layer 40")).toBe("Plate 2 of 3 · Slicing layers");
  });
});