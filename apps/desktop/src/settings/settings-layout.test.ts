import { describe, expect, it } from "vitest";
import { SETTINGS_LOCATION_MAP, filterSections } from "./settings-layout";

describe("settings layout", () => {
  it("finds sections by keyword", () => {
    const hits = filterSections("bambu");
    expect(hits.some((row) => row.id === "accounts")).toBe(true);
  });

  it("documents every legacy surface", () => {
    expect(SETTINGS_LOCATION_MAP.length).toBeGreaterThanOrEqual(8);
  });
});