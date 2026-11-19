/** Client-side manifest parser (mirrors shale_core::slicer_import::parse_import_manifest). */

export function parseImportManifest(text: string): string[] {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (err) {
    throw new Error(`manifest json: ${String(err)}`);
  }
  if (!value || typeof value !== "object") {
    throw new Error("manifest must be an object");
  }
  const presets = (value as { presets?: unknown }).presets;
  if (!Array.isArray(presets)) {
    throw new Error("manifest missing presets array");
  }
  const names: string[] = [];
  for (const item of presets) {
    if (typeof item === "string") {
      names.push(item.trim());
      continue;
    }
    if (item && typeof item === "object" && typeof (item as { name?: unknown }).name === "string") {
      names.push((item as { name: string }).name.trim());
      continue;
    }
    throw new Error("preset entry needs a name");
  }
  return names;
}