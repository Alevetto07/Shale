import type { MaterialInfo, PrinterInfo, QualityInfo, RecentProject } from "./api";
import type { CommandPaletteItem } from "./ui";
import type { Pane } from "./Tabs";
import { UI_STYLES, uiStyleLabel, type UiStyleId } from "./ui-styles";

export interface CommandPaletteSources {
  pane: Pane;
  sliced: boolean;
  recents: RecentProject[];
  printers: PrinterInfo[];
  printerId: string | null;
  qualities: QualityInfo[];
  quality: string | null;
  materials: MaterialInfo[];
  material: string | null;
  uiStyle: UiStyleId;
  onNewProject: () => void;
  onOpenProject: () => void;
  onSlice: () => void;
  onPane: (pane: Pane) => void;
  onPrinter: (id: string) => void;
  onQuality: (q: string) => void;
  onMaterial: (m: string) => void;
  onToggleTheme: () => void;
  onOpenSettings: () => void;
  onOpenRecent: (path: string) => void;
  onClearPlate: () => void;
  hasModels: boolean;
}

const PANE_ORDER: Pane[] = ["home", "prepare", "preview", "device"];

const paneLabel: Record<Pane, string> = {
  home: "Home",
  prepare: "Prepare",
  preview: "Preview",
  device: "Device",
};

export function buildCommandPaletteItems(src: CommandPaletteSources): CommandPaletteItem[] {
  const project: CommandPaletteItem[] = [
    {
      id: "project-new",
      group: "Project",
      label: "New project",
      keywords: ["create", "blank"],
      shortcut: "N",
      onSelect: src.onNewProject,
    },
    {
      id: "project-open",
      group: "Project",
      label: "Open project…",
      keywords: ["file", "load"],
      shortcut: "Ctrl O",
      onSelect: src.onOpenProject,
    },
    {
      id: "project-slice",
      group: "Project",
      label: "Slice",
      keywords: ["gcode", "generate"],
      onSelect: src.onSlice,
    },
    {
      id: "project-clear",
      group: "Project",
      label: "Clear plate",
      keywords: ["empty", "remove parts", "reset"],
      disabled: !src.hasModels,
      onSelect: src.onClearPlate,
    },
  ];

  const navigate: CommandPaletteItem[] = PANE_ORDER.map((pane) => ({
    id: `pane-${pane}`,
    group: "Navigate",
    label: `Go to ${paneLabel[pane]}`,
    keywords: [pane, "switch", "pane", "view"],
    disabled: pane === "preview" && !src.sliced,
    onSelect: () => src.onPane(pane),
  }));

  const appearance: CommandPaletteItem[] = [
    {
      id: "theme-toggle",
      group: "Appearance",
      label: `Toggle theme (now ${uiStyleLabel(src.uiStyle)})`,
      keywords: ["andromeda", "lunar", "saturn", "nebula", "style", "dark", "light"],
      onSelect: src.onToggleTheme,
    },
    {
      id: "settings-open",
      group: "Appearance",
      label: "Open settings",
      keywords: ["preferences", "options"],
      shortcut: "Ctrl ,",
      onSelect: src.onOpenSettings,
    },
  ];

  const profile: CommandPaletteItem[] = [];
  for (const printer of src.printers) {
    profile.push({
      id: `printer-${printer.id}`,
      group: "Printer & profile",
      label: `Printer: ${printer.name}`,
      keywords: [printer.manufacturer, "machine"],
      disabled: printer.id === src.printerId,
      onSelect: () => src.onPrinter(printer.id),
    });
  }
  for (const entry of src.qualities) {
    profile.push({
      id: `quality-${entry.quality_type}`,
      group: "Printer & profile",
      label: `Quality: ${entry.name}`,
      keywords: [entry.quality_type, "preset", "profile"],
      disabled: entry.quality_type === src.quality,
      onSelect: () => src.onQuality(entry.quality_type),
    });
  }
  for (const entry of src.materials) {
    profile.push({
      id: `material-${entry.id}`,
      group: "Printer & profile",
      label: `Material: ${entry.label}`,
      keywords: ["filament", "profile", entry.id],
      disabled: entry.id === src.material,
      onSelect: () => src.onMaterial(entry.id),
    });
  }

  const recent: CommandPaletteItem[] = src.recents.slice(0, 12).map((entry) => ({
    id: `recent-${entry.path}`,
    group: "Recent projects",
    label: entry.name,
    keywords: [entry.path, entry.printer ?? "", "recent", "open"],
    disabled: entry.missing,
    onSelect: () => src.onOpenRecent(entry.path),
  }));

  return [...project, ...navigate, ...appearance, ...profile, ...recent];
}

export function nextUiStyle(current: UiStyleId): UiStyleId {
  const index = UI_STYLES.findIndex((style) => style.id === current);
  const next = UI_STYLES[(index + 1) % UI_STYLES.length];
  return next?.id ?? UI_STYLES[0].id;
}