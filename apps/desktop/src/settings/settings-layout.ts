/** Stable ids for the grouped settings shell (search + sidebar). */

export type SettingsSectionId =
  | "general"
  | "appearance"
  | "printers"
  | "profiles"
  | "slicing"
  | "accounts"
  | "import"
  | "advanced"
  | "about";

export interface SettingsSection {
  id: SettingsSectionId;
  label: string;
  description: string;
  keywords: string[];
}

export const SETTINGS_SECTIONS: SettingsSection[] = [
  {
    id: "general",
    label: "General",
    description: "Engine backend, paths, and desktop behaviour.",
    keywords: ["backend", "engine", "cli", "arcus", "binary", "libs", "setup", "wizard"],
  },
  {
    id: "appearance",
    label: "Appearance",
    description: "Interface style and live plate previews.",
    keywords: ["theme", "style", "andromeda", "saturn", "nebula", "lunar", "ui"],
  },
  {
    id: "printers",
    label: "Printers",
    description: "Default machine from the catalogue.",
    keywords: ["machine", "bambu", "nozzle", "printer", "catalogue"],
  },
  {
    id: "profiles",
    label: "Filaments & profiles",
    description: "Variant, process, and filament defaults.",
    keywords: ["filament", "material", "quality", "process", "variant", "preset"],
  },
  {
    id: "slicing",
    label: "Slicing defaults",
    description: "Cost estimates and per-plate slice settings entry points.",
    keywords: ["slice", "cost", "currency", "override", "g-code"],
  },
  {
    id: "accounts",
    label: "Bambu Lab account",
    description: "Cloud sign-in used for remote printers.",
    keywords: ["cloud", "login", "bambu", "account", "email"],
  },
  {
    id: "import",
    label: "Import",
    description: "Bring presets from other slicers.",
    keywords: ["orca", "prusa", "cura", "bambu studio", "import"],
  },
  {
    id: "advanced",
    label: "Advanced",
    description: "Re-run setup and reset local preferences.",
    keywords: ["reset", "setup", "advanced", "danger"],
  },
  {
    id: "about",
    label: "About",
    description: "Version, licences, and credits.",
    keywords: ["version", "credit", "agpl", "cura", "license"],
  },
];

/** Where each legacy surface moved (for the PR mapping table). */
export const SETTINGS_LOCATION_MAP: { from: string; to: SettingsSectionId }[] = [
  { from: "Start screen → Settings panel → backend select", to: "general" },
  { from: "Start screen → Settings panel → binary/libs", to: "general" },
  { from: "Start screen → Settings panel → UiStylePicker", to: "appearance" },
  { from: "Menu Bar → View → UiStylePicker", to: "appearance" },
  { from: "Start screen → Printers panel", to: "printers" },
  { from: "Menu Bar → Printer (machine / nozzle / process / filament)", to: "profiles" },
  { from: "Menu Bar → Slice settings (ProcessSheet)", to: "slicing" },
  { from: "Prepare sidebar → engine frame", to: "general" },
  { from: "Preview sidebar → cost frame", to: "slicing" },
  { from: "Device pane → Bambu cloud panel", to: "accounts" },
  { from: "Start screen → About panel", to: "about" },
  { from: "Menu → File → Import model", to: "import" },
];

export function filterSections(query: string): SettingsSection[] {
  const q = query.trim().toLowerCase();
  if (!q) return SETTINGS_SECTIONS;
  return SETTINGS_SECTIONS.filter((section) => {
    const hay = `${section.label} ${section.description} ${section.keywords.join(" ")}`.toLowerCase();
    return hay.includes(q);
  });
}