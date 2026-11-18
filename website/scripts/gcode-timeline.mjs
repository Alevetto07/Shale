/**
 * Swimlane timeline from real G-code: segments placed between ;TIME_ELAPSED marks.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const [inPath, outPath] = process.argv.slice(2);
if (!inPath || !outPath) {
  console.error("Usage: node gcode-timeline.mjs <in.gcode> <out.json>");
  process.exit(1);
}

const lines = readFileSync(inPath, "utf8").split("\n");

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
};

let layerIndex = 0;
let curType = "infill";
let lastE = null;
let layerStartT = 0;
let elapsed = 0;
/** @type {{ type: string, layer: number }[]} */
let buffer = [];
/** @type {{ layer: number, t: number }[]} */
const layerTimes = [{ layer: 0, t: 0 }];
/** @type {{ type: string, t0: number, t1: number, layer: number }[]} */
const segments = [];
const laneCounts = {
  "wall-outer": 0,
  "wall-inner": 0,
  skin: 0,
  infill: 0,
  support: 0,
  travel: 0,
};

function flushLayer(endT) {
  const n = buffer.length;
  if (n === 0) {
    layerStartT = endT;
    return;
  }
  const span = Math.max(0.001, endT - layerStartT);
  for (let i = 0; i < n; i++) {
    const m = buffer[i];
    const t0 = layerStartT + (i / n) * span;
    const t1 = layerStartT + ((i + 1) / n) * span;
  segments.push({ type: m.type, t0: round(t0), t1: round(t1), layer: m.layer });
    laneCounts[m.type] = (laneCounts[m.type] ?? 0) + 1;
  }
  buffer = [];
  layerStartT = endT;
}

for (const raw of lines) {
  const line = raw.trim();
  if (!line) continue;

  if (line.startsWith(";LAYER:")) {
    layerIndex = Number(line.slice(7));
    continue;
  }
  if (line.startsWith(";TIME_ELAPSED:")) {
    elapsed = Number(line.slice(14));
    flushLayer(elapsed);
    layerTimes.push({ layer: layerIndex, t: elapsed });
    continue;
  }
  if (line.startsWith(";TYPE:")) {
    const key = line.slice(6).trim();
    curType = typeMap[key] ?? "infill";
    continue;
  }
  if (line.startsWith(";")) continue;

  const g = line.match(/^(G0|G1)\b/i);
  if (!g) continue;

  let ne = lastE;
  const tok = /([XYZEF])(-?\d*\.?\d+)/gi;
  let m;
  while ((m = tok.exec(line))) {
    if (m[1] === "E") ne = Number(m[2]);
  }
  const isExtrude =
    g[1].toUpperCase() === "G1" && ne !== null && lastE !== null && ne > lastE + 1e-6;
  lastE = ne ?? lastE;

  const type = isExtrude ? curType : "travel";
  buffer.push({ type, layer: layerIndex });
}

flushLayer(meta.time_s || elapsed);
if (layerTimes[layerTimes.length - 1]?.t < meta.time_s) {
  layerTimes.push({ layer: layerIndex, t: meta.time_s });
}

const maxSegs = 3500;
const stride = Math.max(1, Math.ceil(segments.length / maxSegs));
const sampled = segments.filter((_, i) => i % stride === 0);

const log = [];
let gi = 0;
for (const line of lines) {
  if (!line.match(/^(G1|;LAYER|;TYPE)/)) continue;
  if (gi++ % 180 !== 0) continue;
  log.push(line.trim().slice(0, 88));
  if (log.length >= 64) break;
}

const lanes = Object.entries(laneCounts).map(([id, count]) => ({ id, count }));

writeFileSync(
  outPath,
  JSON.stringify({
    label: `Real CuraEngine G-code · ${basename(inPath)}`,
    meta: { ...meta, moves: segments.length, layers: layerTimes.length },
    layer_times: layerTimes,
    lanes,
    segments: sampled,
    log,
  }),
);
console.log(`Wrote ${outPath}: ${sampled.length}/${segments.length} segments`);

function round(n) {
  return Math.round(n * 1000) / 1000;
}