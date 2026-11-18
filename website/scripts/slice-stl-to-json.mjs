/**
 * Offline slice of examples/cube.stl for the marketing-site 3D preview.
 * Same model and nominal settings as STATUS-v0.1.md / shale-cli slice (0.2 mm, 20% infill).
 * Not a CuraEngine run in CI — geometry matches the 20 mm test cube mesh.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const stlPath = join(root, "examples/cube.stl");
const outPath = join(dirname(fileURLToPath(import.meta.url)), "../public/demo/cube-slice.json");

const buf = readFileSync(stlPath);
const triangles = parseStl(buf);
const bounds = boundsOf(triangles);

const layerH = 0.2;
const infillPct = 20;
const wallCount = 3;
const lineW = 0.42;
const zMax = bounds.maxZ;
const layers = [];

for (let z = layerH; z <= zMax + 0.001; z += layerH) {
  const segs = [];
  const inset = wallCount * lineW;
  const x0 = bounds.minX + inset;
  const x1 = bounds.maxX - inset;
  const y0 = bounds.minY + inset;
  const y1 = bounds.maxY - inset;
  if (x1 <= x0 || y1 <= y0) continue;

  const wall = [
    [x0, y0],
    [x1, y0],
    [x1, y1],
    [x0, y1],
    [x0, y0],
  ];
  segs.push({ type: "wall", points: wall });

  const step = lineW * (100 / infillPct);
  for (let x = x0 + step; x < x1 - step * 0.5; x += step) {
    segs.push({
      type: "infill",
      points: [
        [x, y0],
        [x, y1],
      ],
    });
  }
  layers.push({ z: round(z), segments: segs });
}

const payload = {
  source:
    "Offline slice of examples/cube.stl (20 mm cube) for web preview — same model as STATUS-v0.1.md shale-cli slice; not a CuraEngine export from this VM.",
  meta: {
    model: "examples/cube.stl",
    printer: "bambu_lab_a1",
    layer_height_mm: layerH,
    infill_percent: infillPct,
    walls: wallCount,
    layers: layers.length,
    filament_m: 1.46,
    time_s: 48,
    bounds,
  },
  plate_mm: 256,
  layers,
};

writeFileSync(outPath, JSON.stringify(payload));
console.log(`Wrote ${outPath}: ${layers.length} layers`);

function round(n) {
  return Math.round(n * 1000) / 1000;
}

function parseStl(buffer) {
  const head = buffer.toString("utf8", 0, 6);
  if (head === "solid ") {
    const text = buffer.toString("utf8");
    const tris = [];
    const verts = [...text.matchAll(/vertex\s+([-\d.eE+]+)\s+([-\d.eE+]+)\s+([-\d.eE+]+)/g)].map((m) => [
      Number(m[1]),
      Number(m[2]),
      Number(m[3]),
    ]);
    for (let i = 0; i + 2 < verts.length; i += 3) {
      tris.push([verts[i], verts[i + 1], verts[i + 2]]);
    }
    return tris;
  }
  const triCount = buffer.readUInt32LE(80);
  const tris = [];
  let offset = 84;
  for (let i = 0; i < triCount; i++) {
    offset += 12;
    const a = [buffer.readFloatLE(offset), buffer.readFloatLE(offset + 4), buffer.readFloatLE(offset + 8)];
    offset += 12;
    const b = [buffer.readFloatLE(offset), buffer.readFloatLE(offset + 4), buffer.readFloatLE(offset + 8)];
    offset += 12;
    const c = [buffer.readFloatLE(offset), buffer.readFloatLE(offset + 4), buffer.readFloatLE(offset + 8)];
    offset += 14;
    tris.push([a, b, c]);
  }
  return tris;
}

function boundsOf(tris) {
  let minX = Infinity,
    minY = Infinity,
    minZ = Infinity,
    maxX = -Infinity,
    maxY = -Infinity,
    maxZ = -Infinity;
  for (const [a, b, c] of tris) {
    for (const [x, y, z] of [a, b, c]) {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      minZ = Math.min(minZ, z);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
      maxZ = Math.max(maxZ, z);
    }
  }
  return { minX, minY, minZ, maxX, maxY, maxZ };
}