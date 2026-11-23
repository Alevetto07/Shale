/** Label position along the track (0–100%) on the unfilled side, away from the thumb. */
export function sliderValueLabelPercent(thumbPercent: number): number {
  const thumb = Math.min(100, Math.max(0, thumbPercent));
  if (thumb <= 0) return 72;
  if (thumb >= 100) return 28;
  const rightSpace = 100 - thumb;
  const leftSpace = thumb;
  if (rightSpace > leftSpace + 4) {
    return thumb + rightSpace * 0.52;
  }
  if (leftSpace > rightSpace + 4) {
    return leftSpace * 0.48;
  }
  return Math.min(94, thumb + rightSpace * 0.58);
}

export function sliderValueOnFill(thumbPercent: number, labelPercent: number): boolean {
  return labelPercent < thumbPercent - 0.5;
}

export function formatSliderLabel(
  value: number,
  inputDigits: number,
  unit: string,
  formatValue?: (value: number) => string,
): string {
  if (formatValue) return formatValue(value);
  if (!Number.isFinite(value)) return "";
  const core = inputDigits <= 0 ? String(Math.round(value)) : value.toFixed(inputDigits);
  if (!unit) return core;
  const glue = unit === "°" || unit === "%" ? "" : " ";
  return `${core}${glue}${unit}`;
}