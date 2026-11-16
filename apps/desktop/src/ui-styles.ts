import andromedaPlate from "./field/andromeda.jpg";
import lunarPlate from "./field/lunar.jpg";
import nebulaPlate from "./field/nebula.jpg";
import saturnPlate from "./field/saturn.jpg";
import type { PlateTune } from "./field/plate-tune";

export const UI_STYLE_STORAGE_KEY = "shale.ui-style";

/** Internal theme ids; must match `data-ui-theme` and `themes/<id>.css`. */
export const UI_STYLE_IDS = ["deep-field", "saturn", "nebula", "lunar"] as const;
export type UiStyleId = (typeof UI_STYLE_IDS)[number];

export const DEFAULT_UI_STYLE_ID: UiStyleId = "deep-field";

export interface PlateCredit {
  name: string;
  object: string;
  credit: string;
  release: string;
  page: string;
  licence: string;
}

export interface UiStyleDefinition {
  id: UiStyleId;
  /** User-visible style name (astronomy-themed). */
  label: string;
  plateImage: string;
  plateCredit: PlateCredit;
  /** Swatch colours for the style picker preview. */
  swatch: { bg: string; panel: string; accent: string; ink: string };
  /** RGB background behind the ASCII plate painter. */
  plateBackground: readonly [number, number, number];
  plateTune?: PlateTune;
}

export const UI_STYLES: readonly UiStyleDefinition[] = [
  {
    id: "deep-field",
    label: "Andromeda",
    plateImage: andromedaPlate,
    plateCredit: {
      name: "Andromeda Galaxy",
      object: "M31",
      credit: "NASA/JPL-Caltech",
      release: "16 May 2012",
      page: "https://images.nasa.gov/details/PIA15416",
      licence: "Not subject to copyright in the United States",
    },
    swatch: { bg: "#06070b", panel: "#0b0d14", accent: "#b9b3ff", ink: "#e9e8f2" },
    plateBackground: [6, 7, 11],
  },
  {
    id: "saturn",
    label: "Saturn",
    plateImage: saturnPlate,
    plateCredit: {
      name: "Saturn during equinox",
      object: "Saturn",
      credit: "NASA/JPL/Space Science Institute",
      release: "22 September 2009",
      page: "https://commons.wikimedia.org/wiki/File:Saturn_during_Equinox.jpg",
      licence: "Public domain (NASA)",
    },
    swatch: { bg: "#0c0906", panel: "#15100a", accent: "#e8b86a", ink: "#f2e8dc" },
    plateBackground: [12, 9, 6],
    plateTune: {
      lumaPower: 0.5,
      lumaGain: 1.08,
      lumaFloor: 0.04,
      fitScaleMul: 0.95,
    },
  },
  {
    id: "nebula",
    label: "Nebula",
    plateImage: nebulaPlate,
    plateCredit: {
      name: "Crab Nebula",
      object: "M1",
      credit: "NASA, ESA, J. Hester and A. Loll (Arizona State University)",
      release: "1 November 2005",
      page: "https://commons.wikimedia.org/wiki/File:Crab_Nebula.jpg",
      licence: "Public domain (NASA)",
    },
    swatch: { bg: "#09060e", panel: "#120a1a", accent: "#e07aff", ink: "#efe6f8" },
    plateBackground: [9, 6, 14],
    plateTune: {
      lumaPower: 0.48,
      lumaGain: 0.92,
      lumaFloor: 0.05,
      fitScaleMul: 0.9,
      fitBiasX: -0.11,
      fitBiasY: -0.02,
      glyphFrom: "pixel",
      twinklePeriod: 0,
    },
  },
  {
    id: "lunar",
    label: "Lunar",
    plateImage: lunarPlate,
    plateCredit: {
      name: "Moon near first quarter",
      object: "Moon",
      credit: "Luc Viatour / Lviatour (half-phase derivative for Shale)",
      release: "2009",
      page: "https://commons.wikimedia.org/wiki/File:Full_Moon_Luc_Viatour.jpg",
      licence: "CC BY-SA 3.0",
    },
    swatch: { bg: "#eef0f4", panel: "#ffffff", accent: "#1d5fd6", ink: "#141820" },
    plateBackground: [238, 240, 244],
    plateTune: {
      lumaPower: 0.55,
      lumaGain: 1.12,
      lumaFloor: 0.09,
      emptyPixelMax: 20,
      sampleInset: 2,
      fitMode: "contain",
      fitScaleMul: 0.88,
      fitBiasX: -0.02,
      glyphFrom: "graphite",
      glyphTintLo: [108, 118, 136],
      glyphTintHi: [42, 50, 68],
      twinklePeriod: 0,
    },
  },
] as const;

const byId = new Map(UI_STYLES.map((style) => [style.id, style]));

export function isUiStyleId(value: string): value is UiStyleId {
  return (UI_STYLE_IDS as readonly string[]).includes(value);
}

export function uiStyleById(id: UiStyleId): UiStyleDefinition {
  return byId.get(id) ?? byId.get(DEFAULT_UI_STYLE_ID)!;
}

/** Legacy export: default theme id. */
export const UI_THEME_ID = DEFAULT_UI_STYLE_ID;

function styleFromUrl(): UiStyleId | null {
  if (typeof window === "undefined") return null;
  const raw = new URLSearchParams(window.location.search).get("uiStyle");
  if (!raw || !isUiStyleId(raw)) return null;
  return raw;
}

export function readUiStyle(): UiStyleId {
  const fromUrl = styleFromUrl();
  if (fromUrl) return fromUrl;
  try {
    const raw = window.localStorage.getItem(UI_STYLE_STORAGE_KEY);
    if (raw && isUiStyleId(raw)) return raw;
  } catch {
    // private mode or blocked storage
  }
  return DEFAULT_UI_STYLE_ID;
}

export function writeUiStyle(id: UiStyleId): void {
  try {
    window.localStorage.setItem(UI_STYLE_STORAGE_KEY, id);
  } catch {
    // ignore
  }
}

/** Apply theme to the document root without waiting for React. */
export function applyUiStyle(id: UiStyleId): void {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.uiTheme = id;
  const style = uiStyleById(id);
  document.documentElement.style.colorScheme =
    id === "lunar" ? "light" : "dark";
  document.documentElement.style.setProperty("--viewport-bg", style.swatch.bg);
}

export function uiStylePlateImage(id: UiStyleId): string {
  return uiStyleById(id).plateImage;
}