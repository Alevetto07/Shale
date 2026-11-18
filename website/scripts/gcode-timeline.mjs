/**
 * Build a compact print-time timeline from CuraEngine G-code (for swimlane UI).
 * Usage: node gcode-timeline.mjs <in.gcode> <out.json>
 */
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const [inPath, outPath] = process.argv.slice(2);
if (!inPath || !outPath) {
  console.error("Usage: node gcode-timeline.mjs <in.gcode> <out.json>");
  process.exit(1);
}

const text = readFileSync(inPath, "utf8");
const lines = text.split("\n");

let meta = { filament_m: 0, time_s: 0, layer_height_mm: 0.2 };
for (const line of lines) {
  if (line.startsWith(";TIME:")) meta.time_s = Number(line.slice(6));
  if (line.startsWith(";Filament used:")) meta.filament_m = Number(line.slice(15).replace(/m$/, ""));
  if (line.startsWith(";Layer height:")) meta.layer_height_mm = Number(line.slice(14));
}

const typeMap = {
  "WALL-OUTER": "wall-outer",
  "WALL-INNER": "wall-inner",
  SKIN: "skin",
  FILL: "infill",
  SUPPORT: "support",
  SKIRT: "skirt",
};

const lanes = {
  "wall-outer": 0,
  "wall-inner": 0,
  skin: 0,
  infill: 0,
  support: 0,
  travel: 0,
};
let layerIndex = 0;
let curType = "travel";
let moves = 0;
const log = [];
let x = 0,
  y = 0,
  lastE = null;

function mapType(t) {
  return typeMap[t.replace(";TYPE:", "").trim()] ?? "travel";
}

for (const raw of lines) {
  const line = raw.trim();
  if (!line) continue;
  if (line.startsWith(";LAYER:")) {
    layerIndex = Number(line.slice(7));
    if (log.length < 120 && log.length % 3 === 0) log.push(line);
    continue;
  }
  if (line.startsWith(";TYPE:")) {
    curType = mapType(line);
    continue;
  }
  if (line.startsWith(";TIME:") || line.startsWith(";Filament")) continue;
  if (line.startsWith(";") && log.length < 40 && moves % 400 === 12) log.push(line.slice(0, 96));
  if (!line.match(/^(G0|G1)\b/i)) continue;

  let ne = lastE;
  const tok = /([XYZEF])(-?\d*\.?\d+)/gi;
  let m;
  while ((m = tok.exec(line))) {
    if (m[1] === "E") ne = Number(m[2]);
  }
  const isExtrude =
    /^G1/i.test(line) && ne !== null && lastE !== null && ne > lastE + 1e-6;
  lastE = ne ?? lastE;
  moves += 1;

  const type = isExtrude ? (curType === "travel" ? "infill" : curType) : "travel";
  if (type in lanes) lanes[type] += 1;

  if (log.length < 100 && moves % 180 === 0) {
    const t = meta.time_s > 0 ? ((layerIndex / 120) * meta.time_s) | 0 : layerIndex;
    const mm = String(Math.floor(t / 60)).padStart(2, "0");
    const ss = String(t % 60).padStart(2, "0");
    log.push(`+${mm}:${ss} L${layerIndex} · ${type} · ${line.slice(0, 72)}`);
  }
}

const total = Object.values(lanes).reduce((a, b) => a + b, 0) || 1;
const laneList = Object.entries(lanes).map(([id, count]) => ({
  id,
  count,
  fraction: count / total,
}));

const payload = {
  label: `Real CuraEngine G-code · ${basename(inPath)}`,
  meta: {
    ...meta,
    moves,
    layers: layerIndex + 1,
  },
  lanes: laneList,
  log: log.slice(0, 96),
};

writeFileSync(outPath, JSON.stringify(payload));
console.log(`Wrote ${outPath} · ${moves} moves · ${laneList.length} lanes`);