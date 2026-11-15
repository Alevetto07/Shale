import type { PrinterInfo, QualityInfo } from "./api";

/** Pick the preset whose label is "{nozzle} Standard" when we need a default. */
export function defaultQualityType(qualities: QualityInfo[], printer?: PrinterInfo): string | null {
  return (
    qualities.find((item) => item.name.includes("0.20 mm Standard"))?.quality_type ??
    qualities.find((item) => item.quality_type === printer?.preferred_quality_type)?.quality_type ??
    qualities.find((item) => item.name.toLowerCase().includes("standard"))?.quality_type ??
    qualities.find((item) => item.quality_type === "draft")?.quality_type ??
    qualities[0]?.quality_type ??
    null
  );
}

export function isKnownQuality(qualities: QualityInfo[], qualityType: string | null | undefined): boolean {
  return typeof qualityType === "string" && qualities.some((item) => item.quality_type === qualityType);
}

/**
 * Pick a process preset when the list (re)loads. Never downgrades an explicit
 * user or project choice just because the catalogue finished loading.
 */
export function pickQualityType(
  qualities: QualityInfo[],
  options: {
    pending?: string | null;
    current?: string | null;
    printer?: PrinterInfo;
    nozzleChanged?: boolean;
  },
): string | null {
  const { pending, current, printer, nozzleChanged = false } = options;
  if (!nozzleChanged && isKnownQuality(qualities, pending)) return pending!;
  if (isKnownQuality(qualities, current)) return current!;
  return defaultQualityType(qualities, printer);
}