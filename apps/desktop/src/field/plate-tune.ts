/** Per-style sampling and glyph colouring for the ASCII plate painter. */
export interface PlateTune {
  /** Luminance curve (default 0.45). Higher = darker field overall. */
  lumaPower?: number;
  /** Extra gain applied before the glyph ramp (default 1). */
  lumaGain?: number;
  /** Luminance below this produces no glyph (0–1). */
  lumaFloor?: number;
  /** Multiplier on cover-fit scale (default 1.03). */
  fitScaleMul?: number;
  /** Shift the fitted image horizontally, as a fraction of canvas width. */
  fitBiasX?: number;
  fitBiasY?: number;
  /** Where glyph RGB comes from. */
  glyphFrom?: "pixel" | "graphite" | "nebula";
  /** Dark / light graphite ink for mono modes. */
  glyphTintLo?: readonly [number, number, number];
  glyphTintHi?: readonly [number, number, number];
  /**
   * Twinkle period: one star roughly every N cells. 0 disables twinkle.
   * Default 97.
   */
  twinklePeriod?: number;
  /** When true, samples within a few steps of `backgroundRgb` are treated as empty. */
  matchBackgroundEmpty?: boolean;
}

export const DEFAULT_PLATE_TUNE: PlateTune = {};