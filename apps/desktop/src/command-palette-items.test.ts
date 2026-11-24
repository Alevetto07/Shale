import { describe, expect, it, vi } from "vitest";
import { buildCommandPaletteItems } from "./command-palette-items";

describe("buildCommandPaletteItems", () => {
  it("includes recent projects and disables preview without a slice", () => {
    const items = buildCommandPaletteItems({
      pane: "prepare",
      sliced: false,
      recents: [{ name: "Bracket", path: "/tmp/bracket.shale", saved_at_ms: Date.now(), missing: false, printer: "A1" }],
      printers: [
        {
          id: "a1",
          name: "A1",
          manufacturer: "Bambu",
          source: "cura",
          preferred_quality_type: "draft",
          preferred_variant: "0.4",
          preferred_material: "pla",
        },
      ],
      printerId: "a1",
      qualities: [{ quality_type: "draft", name: "Draft" }],
      quality: "draft",
      materials: [{ id: "pla", label: "PLA", color: "#fff", diameter: 1.75, density: 1.24 }],
      material: "pla",
      uiStyle: "deep-field",
      hasModels: true,
      onNewProject: vi.fn(),
      onOpenProject: vi.fn(),
      onSlice: vi.fn(),
      onClearPlate: vi.fn(),
      onPane: vi.fn(),
      onPrinter: vi.fn(),
      onQuality: vi.fn(),
      onMaterial: vi.fn(),
      onToggleTheme: vi.fn(),
      onOpenSettings: vi.fn(),
      onOpenRecent: vi.fn(),
      nonplanarMode: "off",
      onNonplanarMode: vi.fn(),
      onToggleStrict: vi.fn(),
      onOpenNonplanar: vi.fn(),
      onExportGcode: vi.fn(),
      onArrange: vi.fn(),
      onUndo: vi.fn(),
      onRedo: vi.fn(),
      canUndo: false,
      canRedo: false,
      onUiStyle: vi.fn(),
      liveSlice: true,
      onToggleLiveSlice: vi.fn(),
    });
    const preview = items.find((item) => item.id === "pane-preview");
    expect(preview?.disabled).toBe(true);
    const recent = items.find((item) => item.id === "recent-/tmp/bracket.shale");
    expect(recent?.label).toBe("Bracket");
    expect(items.some((item) => item.id === "nonplanar-auto")).toBe(true);
    expect(items.some((item) => item.label === "Theme: Andromeda")).toBe(true);
    expect(items.some((item) => /deep field/i.test(item.label))).toBe(false);
  });
});