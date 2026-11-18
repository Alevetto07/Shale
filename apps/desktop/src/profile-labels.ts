import type { VariantInfo } from "./api";

/** Nozzle row: avoid `0.4mm Nozzle (0.4 mm)` when the name already states the size. */
export function variantOptionLabel(variant: VariantInfo): string {
  const { name, nozzle_size: size } = variant;
  if (size == null) return name;
  const compact = size.toFixed(2).replace(/\.?0+$/, "");
  const compactOne = size.toFixed(1);
  if (
    name.includes(`${compact} mm`) ||
    name.includes(`${compactOne} mm`) ||
    name.includes(`${compact}mm`) ||
    name.includes(`${compactOne}mm`)
  ) {
    return name;
  }
  return `${name} (${size} mm)`;
}