import { describe, expect, it } from "vitest";
import { quickSettingsView, userSettingsForEngine } from "./engine-settings";

const fallbacks = {
  layerHeight: 0.2,
  walls: 2,
  infill: 20,
  pattern: "grid",
  supports: false,
  speed: 50,
  nozzle: 200,
  bed: 60,
};

describe("userSettingsForEngine", () => {
  it("sends layer_height only when overridden", () => {
    expect(userSettingsForEngine({}).layer_height).toBeUndefined();
    expect(userSettingsForEngine({ layer_height: "0.16" }).layer_height).toBe(0.16);
    expect(userSettingsForEngine({ speed_print: "80" }).layer_height).toBeUndefined();
    expect(userSettingsForEngine({ speed_print: "80" }).extra).toContainEqual(["speed_print", "80"]);
  });

  it("sends non-planar off unless the plate asks otherwise", () => {
    expect(userSettingsForEngine({}).nonplanar).toEqual({
      mode: "off",
      max_angle_deg: 10,
      strict: false,
      underside_bands: false,
    });
    expect(
      userSettingsForEngine(
        {},
        { mode: "auto", max_angle_deg: 40, strict: true, underside_bands: true },
      ).nonplanar,
    ).toEqual({
      mode: "auto",
      max_angle_deg: 10,
      strict: true,
      underside_bands: true,
    });
  });
});

describe("quickSettingsView", () => {
  it("shows profile until overridden", () => {
    const profile = { layer_height: "0.2", speed_print: "100" };
    expect(quickSettingsView(profile, {}, fallbacks).layerHeight).toBe(0.2);
    expect(quickSettingsView(profile, {}, fallbacks).speed).toBe(100);
    expect(quickSettingsView(profile, { layer_height: "0.16" }, fallbacks).layerHeight).toBe(0.16);
    expect(quickSettingsView(profile, { layer_height: "0.16" }, fallbacks).speed).toBe(100);
  });
});