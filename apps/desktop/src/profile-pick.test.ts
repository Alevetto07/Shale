import { describe, expect, it } from "vitest";
import { defaultQualityType, pickQualityType } from "./profile-pick";
import type { QualityInfo } from "./api";

const qualities: QualityInfo[] = [
  { quality_type: "0_12mm_fine_bbl_a1", name: "0.12 mm Fine" },
  { quality_type: "0_20mm_standard_bbl_a1", name: "0.20 mm Standard" },
  { quality_type: "0_28mm_extra_draft_bbl_a1", name: "0.28 mm Extra Draft" },
];

describe("pickQualityType", () => {
  it("keeps the current preset when the catalogue reloads", () => {
    expect(
      pickQualityType(qualities, {
        current: "0_20mm_standard_bbl_a1",
        nozzleChanged: false,
      }),
    ).toBe("0_20mm_standard_bbl_a1");
  });

  it("restores a pending preset from a project on open", () => {
    expect(
      pickQualityType(qualities, {
        pending: "0_20mm_standard_bbl_a1",
        current: "0_12mm_fine_bbl_a1",
        nozzleChanged: false,
      }),
    ).toBe("0_20mm_standard_bbl_a1");
  });

  it("defaults to Standard instead of the first list entry", () => {
    expect(defaultQualityType(qualities)).toBe("0_20mm_standard_bbl_a1");
    expect(
      pickQualityType(qualities, {
        current: null,
        nozzleChanged: true,
      }),
    ).toBe("0_20mm_standard_bbl_a1");
  });
});