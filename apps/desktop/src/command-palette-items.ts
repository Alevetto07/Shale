import type { MaterialInfo, NonplanarModeName, PrinterInfo, QualityInfo, RecentProject } from "./api";
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
  onImport: () => void;
  onSlice: () => void;
  onOpenSliceSettings: () => void;
  onPane: (pane: Pane) => void;
  onPrinter: (id: string) => void;
  onQuality: (q: string) => void;
  onMaterial: (m: string) => void;
  onToggleTheme: () => void;
  onOpenSettings: () => void;
  onOpenRecent: (path: string) => void;
  onClearPlate: () => void;
  hasModels: boolean;
  nonplanarMode: NonplanarModeName;
  onNonplanarMode: (mode: NonplanarModeName) => void;
  onToggleStrict: () => void;
  onOpenNonplanar: () => void;
  onExportGcode: () => void;
  onArrange: () => void;
  onUndo: () => void;
  onRedo: () => void;
  canUndo: boolean;
  canRedo: boolean;
  onUiStyle: (id: UiStyleId) => void;
  liveSlice: boolean;
  onToggleLiveSlice: () => void;
}

const PANE_ORDER: Pane[] = ["home", "models", "prepare", "preview", "device"];

const paneLabel: Record<Pane, string> = {
  home: "Home",
  models: "Models",
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
      id: "project-import",
      group: "Project",
      label: "Import model…",
      keywords: ["stl", "3mf", "step", "file"],
      shortcut: "Ctrl I",
      onSelect: src.onImport,
    },
    {
      id: "project-slice-settings",
      group: "Project",
      label: "Slice settings",
      keywords: ["process", "quality", "temperature", "non-planar"],
      onSelect: src.onOpenSliceSettings,
    },
    {
      id: "project-slice",
      group: "Project",
      label: "Slice",
      keywords: ["gcode", "generate"],
      onSelect: src.onSlice,
    },
    {
      id: "project-export",
      group: "Project",
      label: "Export G-code",
      keywords: ["save", "download"],
      disabled: !src.sliced,
      onSelect: src.onExportGcode,
    },
    {
      id: "project-arrange",
      group: "Project",
      label: "Arrange plate",
      keywords: ["layout", "pack"],
      disabled: !src.hasModels,
      onSelect: src.onArrange,
    },
    {
      id: "project-undo",
      group: "Project",
      label: "Undo",
      keywords: ["back"],
      shortcut: "Ctrl Z",
      disabled: !src.canUndo,
      onSelect: src.onUndo,
    },
    {
      id: "project-redo",
      group: "Project",
      label: "Redo",
      keywords: ["forward"],
      disabled: !src.canRedo,
      onSelect: src.onRedo,
    },
    {
      id: "project-live-slice",
      group: "Project",
      label: src.liveSlice ? "Background refresh: on" : "Background refresh: off",
      keywords: ["auto slice", "debounce", "live"],
      onSelect: src.onToggleLiveSlice,
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

  const nonplanar: CommandPaletteItem[] = [
    {
      id: "nonplanar-open",
      group: "Non-planar",
      label: "Show non-planar settings",
      keywords: ["curved", "auto", "angle"],
      onSelect: src.onOpenNonplanar,
    },
    ...(["off", "curved-tops", "auto"] as const).map((mode) => ({
      id: `nonplanar-${mode}`,
      group: "Non-planar",
      label: mode === "off" ? "Non-planar: off" : mode === "auto" ? "Non-planar: auto" : "Non-planar: curved tops",
      keywords: ["curved layers", "nonplanar", mode],
      disabled: src.nonplanarMode === mode,
      onSelect: () => src.onNonplanarMode(mode),
    })),
    {
      id: "nonplanar-strict",
      group: "Non-planar",
      label: "Toggle strict curved slice",
      keywords: ["fallback", "refuse"],
      onSelect: src.onToggleStrict,
    },
  ];

  const appearance: CommandPaletteItem[] = [
    {
      id: "theme-toggle",
      group: "Appearance",
      label: `Toggle theme (now ${uiStyleLabel(src.uiStyle)})`,
      keywords: ["andromeda", "lunar", "saturn", "nebula", "style", "dark", "light"],
      onSelect: src.onToggleTheme,
    },
    ...UI_STYLES.map((style) => ({
      id: `theme-${style.id}`,
      group: "Appearance",
      label: `Theme: ${style.label}`,
      keywords: ["appearance", style.label.toLowerCase()],
      disabled: style.id === src.uiStyle,
      onSelect: () => src.onUiStyle(style.id),
    })),
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

  return [...project, ...navigate, ...nonplanar, ...appearance, ...profile, ...recent];
}

export function nextUiStyle(current: UiStyleId): UiStyleId {
  const index = UI_STYLES.findIndex((style) => style.id === current);
  const next = UI_STYLES[(index + 1) % UI_STYLES.length];
  return next?.id ?? UI_STYLES[0].id;
}