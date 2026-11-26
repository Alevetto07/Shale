import { DEFAULT_NONPLANAR, type NonplanarSettings, type SliceSettings } from "./api";

/** Cura keys bound to the process sheet's quick rows. */
export const QUICK_SETTING_KEYS = {
  layerHeight: "layer_height",
  walls: "wall_line_count",
  infill: "infill_sparse_density",
  pattern: "infill_pattern",
  supports: "support_enable",
  speed: "speed_print",
  nozzle: "material_print_temperature",
  bed: "material_bed_temperature",
} as const;

export type QuickSettingId = keyof typeof QUICK_SETTING_KEYS;

function parseNumber(raw: string | undefined): number | undefined {
  if (raw === undefined || raw === "") return undefined;
  const value = Number(raw);
  return Number.isFinite(value) ? value : undefined;
}

function parseBool(raw: string | undefined): boolean | undefined {
  if (raw === undefined || raw === "") return undefined;
  const lower = raw.toLowerCase();
  if (lower === "1" || lower === "true") return true;
  if (lower === "0" || lower === "false") return false;
  return undefined;
}

/** Effective value for a quick row: override wins, then profile, then fallback. */
export function quickFieldValue(
  key: string,
  profile: Record<string, string>,
  overrides: Record<string, string>,
  fallback: string,
): string {
  if (key in overrides) return overrides[key] ?? "";
  return profile[key] ?? fallback;
}

export function quickSettingsView(
  profile: Record<string, string>,
  overrides: Record<string, string>,
  fallbacks: {
    layerHeight: number;
    walls: number;
    infill: number;
    pattern: string;
    supports: boolean;
    speed: number;
    nozzle: number;
    bed: number;
  },
) {
  const layerHeight =
    parseNumber(quickFieldValue(QUICK_SETTING_KEYS.layerHeight, profile, overrides, "")) ?? fallbacks.layerHeight;
  const walls =
    parseNumber(quickFieldValue(QUICK_SETTING_KEYS.walls, profile, overrides, "")) ?? fallbacks.walls;
  const infill =
    parseNumber(quickFieldValue(QUICK_SETTING_KEYS.infill, profile, overrides, "")) ?? fallbacks.infill;
  const pattern = quickFieldValue(QUICK_SETTING_KEYS.pattern, profile, overrides, fallbacks.pattern);
  const supportRaw = quickFieldValue(QUICK_SETTING_KEYS.supports, profile, overrides, "");
  const supports = parseBool(supportRaw) ?? fallbacks.supports;
  const speed =
    parseNumber(quickFieldValue(QUICK_SETTING_KEYS.speed, profile, overrides, "")) ?? fallbacks.speed;
  const nozzle =
    parseNumber(quickFieldValue(QUICK_SETTING_KEYS.nozzle, profile, overrides, "")) ?? fallbacks.nozzle;
  const bed =
    parseNumber(quickFieldValue(QUICK_SETTING_KEYS.bed, profile, overrides, "")) ?? fallbacks.bed;
  return { layerHeight, walls, infill, pattern, supports, speed, nozzle, bed };
}

function clampAngle(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_NONPLANAR.max_angle_deg;
  return Math.min(10, Math.max(0.5, value));
}

/** What the slicer should receive: only keys the user has explicitly overridden. */
export function userSettingsForEngine(
  scoped: Record<string, string>,
  nonplanar: NonplanarSettings = DEFAULT_NONPLANAR,
): SliceSettings {
  const entries = Object.entries(scoped).filter(([, value]) => value !== "");
  const mode = nonplanar.mode === "auto" || nonplanar.mode === "curved-tops" ? nonplanar.mode : "off";
  return {
    layer_height: parseNumber(scoped.layer_height),
    infill_sparse_density: parseNumber(scoped.infill_sparse_density),
    support_enable: parseBool(scoped.support_enable),
    material_print_temperature: parseNumber(scoped.material_print_temperature),
    material_bed_temperature: parseNumber(scoped.material_bed_temperature),
    extra: entries,
    nonplanar: {
      mode,
      max_angle_deg: clampAngle(nonplanar.max_angle_deg),
      strict: nonplanar.strict,
      underside_bands: mode === "auto" && nonplanar.underside_bands,
    },
  };
}

/** Non-planar controls from a saved project. Missing fields stay at the defaults. */
export function nonplanarFromSliceSettings(settings: SliceSettings): NonplanarSettings {
  const raw = settings.nonplanar;
  if (!raw) return { ...DEFAULT_NONPLANAR };
  const mode = raw.mode === "auto" || raw.mode === "curved-tops" ? raw.mode : "off";
  return {
    mode,
    max_angle_deg: clampAngle(raw.max_angle_deg),
    strict: !!raw.strict,
    underside_bands: !!raw.underside_bands,
  };
}

/** Rebuild plate overrides from a saved project file. */
export function overridesFromSliceSettings(settings: SliceSettings): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of settings.extra) {
    if (value !== "") out[key] = value;
  }
  if (settings.layer_height != null) out.layer_height = String(settings.layer_height);
  if (settings.infill_sparse_density != null) out.infill_sparse_density = String(settings.infill_sparse_density);
  if (settings.support_enable != null) out.support_enable = settings.support_enable ? "1" : "0";
  if (settings.material_print_temperature != null) {
    out.material_print_temperature = String(settings.material_print_temperature);
  }
  if (settings.material_bed_temperature != null) {
    out.material_bed_temperature = String(settings.material_bed_temperature);
  }
  return out;
}