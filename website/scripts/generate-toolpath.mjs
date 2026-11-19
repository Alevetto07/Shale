/**
 * Synthetic 10 mm cube toolpath for the marketing-site viewer.
 * Geometry matches docs/verification.md (0.2 mm layers, ~50 layers for demo).
 */
import { writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "public/demo/toolpath-cube.json");

const layerH = 0.2;
const layers = 48;
const half = 5;
const cx = 128;
const cy = 128;

const layerData = [];

for (let i = 0; i < layers; i++) {
  const z = layerH * (i + 1);
  const inset = i < 4 ? half - i * 0.15 : half - 0.6;
  const wall = [
    [cx - inset, cy - inset],
    [cx + inset, cy - inset],
    [cx + inset, cy + inset],
    [cx - inset, cy + inset],
    [cx - inset, cy - inset],
  ];
  const infill = [];
  const step = 1.2;
  for (let x = cx - inset + step; x < cx + inset - step; x += step) {
    infill.push([
      [x, cy - inset + 0.35],
      [x, cy + inset - 0.35],
    ]);
  }
  layerData.push({ z, walls: [wall], infill });
}

const payload = {
  source:
    "Synthetic paths for a 10 mm cube — same proportions as verification.md A1 / 0.2 mm PLA slice; not a live CuraEngine export.",
  layer_height_mm: layerH,
  layers: layerData,
};

writeFileSync(out, JSON.stringify(payload));
console.log(`Wrote ${out} (${layers} layers)`);