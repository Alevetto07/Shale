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
