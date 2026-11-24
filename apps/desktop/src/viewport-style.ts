/// Colour and view choices shared by the preview and its legend.
///
/// These live outside `Viewport.tsx` so the legend can name a swatch without
/// pulling three.js into the first paint. The heavy canvas loads after the shell.

export type TransformMode = "translate" | "rotate" | "scale";
export type ViewStyle = "solid" | "wire" | "ascii";

/// What the toolpath is coloured by. "type" is what the lines are (wall, skin,
/// infill); "speed" is how fast they are laid down. "slope" paints curved
/// segments from a dim ink to the theme accent.
export type ColourMode = "type" | "speed" | "slope";

/** Matches `shale_nonplanar::CURVED_EXTRUSION_MIN_SLOPE_DEG`. */
export const CURVED_SLOPE_MIN_DEG = 0.05;

/// Slow to fast in one hue: the theme has a single accent, so the ramp runs
/// from a dim grey to that accent rather than through a thermal camera's
/// colours. Five stops is enough for a line to be placed by eye.
export const SPEED_STOPS = ["#474a5e", "#625d7e", "#7b74a3", "#9a93d2", "#b9b3ff"];

export function speedColour(feed: number, low: number, high: number): string {
  if (!(high > low)) return SPEED_STOPS[SPEED_STOPS.length - 1];
  const span = high - low;
  const place = Math.min(1, Math.max(0, (feed - low) / span));
  const index = Math.min(SPEED_STOPS.length - 1, Math.floor(place * SPEED_STOPS.length));
  return SPEED_STOPS[index];
}

/// The colour each kind of line is drawn in. The legend has to name the same
/// colour the plate is painted with. Nine kinds have to be told apart at a
/// glance on a dark plate, which one accent cannot do on its own: these are
/// muted hues at the same lightness, so they read as one palette and still
/// differ. Travel is deliberately almost invisible and is hidden until asked for.
/**
 * Slope colour from the active theme. The dim end is the background with a
 * little of the accent mixed in, and the bright end is the accent itself.
 * Using the text colour as the dim end made Andromeda a pale-on-pale ramp.
 */
/** Read the live theme tokens, so a style change recolours the ramp without a new palette. */
export function slopeRampFromDocument(): { dim: string; accent: string } {
  const styles = getComputedStyle(document.documentElement);
  const accent = styles.getPropertyValue("--accent").trim() || "#b9b3ff";
  const background = styles.getPropertyValue("--bg").trim() || "#06070b";
  return slopeRamp(accent, background);
}

export function slopeRamp(accent: string, background: string): { dim: string; accent: string } {
  return { dim: mixHex(background, accent, 0.42), accent };
}

function mixHex(from: string, to: string, amount: number): string {
  const left = parseHex(from);
  const right = parseHex(to);
  if (!left || !right) return to;
  const channel = (start: number, end: number) =>
    Math.round(start + (end - start) * Math.min(1, Math.max(0, amount)));
  return (
    "#" +
    [0, 1, 2]
      .map((index) => channel(left[index], right[index]).toString(16).padStart(2, "0"))
      .join("")
  );
}

function parseHex(value: string): [number, number, number] | null {
  const hex = value.trim().replace("#", "");
  if (hex.length !== 6) return null;
  const number = Number.parseInt(hex, 16);
  if (!Number.isFinite(number)) return null;
  return [(number >> 16) & 255, (number >> 8) & 255, number & 255];
}

/** Relative luminance, 0–1, for checking a ramp still separates its ends. */
/** How many vertices to draw for layers 0..layer. Later layers stay in the buffer. */
export function verticesThroughLayer(counts: ArrayLike<number>, layer: number): number {
  if (counts.length === 0) return 0;
  const index = Math.min(counts.length - 1, Math.max(0, Math.floor(layer)));
  return counts[index] ?? 0;
}

export function relativeLuminance(hex: string): number {
  const channels = parseHex(hex);
  if (!channels) return 0;
  const linear = channels.map((channel) => {
    const unit = channel / 255;
    return unit <= 0.04045 ? unit / 12.92 : ((unit + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

export const KIND_COLOR: Record<string, string> = {
  "wall-outer": "#e9e8f2",
  "wall-inner": "#b9b3ff",
  skin: "#7fb0c6",
  infill: "#6f9a7c",
  support: "#c08a5a",
  "support-interface": "#e0b07a",
  skirt: "#8d7fbb",
  brim: "#b98fae",
  raft: "#6b6f7e",
  "prime-tower": "#9c9a6a",
  travel: "#23252c",
  extrusion: "#a39fb4",
};
