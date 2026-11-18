/**
 * 2.5D plane-cut slicer for marketing previews (not CuraEngine).
 * Produces trim-friendly JSON for the site 3D viewer.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const inRel = process.argv[2] ?? "examples/t-wing.stl";
const outRel = process.argv[3] ?? "website/public/demo/demo-slice.json";
const stlPath = join(root, inRel);
const outPath = join(root, outRel);

const buf = readFileSync(stlPath);
const triangles = parseStl(buf);
const bounds = boundsOf(triangles);
const layerH = 0.2;
const infillPct = 20;
const wallCount = 3;
const lineW = 0.42;

const layers = [];
for (let z = layerH; z <= bounds.maxZ + 0.001; z += layerH) {
  const raw = sliceAtZ(triangles, z);
  if (!raw.length) continue;
  const segs = [];

  for (const loop of chainSegments(raw)) {
    if (loop.length < 3) continue;
    const closed = closeLoop(loop);
    segs.push({ type: "wall", points: closed.map(([x, y]) => [round(x), round(y)]) });
  }

  const ib = insetBounds(bounds, z, wallCount * lineW);
  if (ib && segs.length) {
    const step = lineW * (100 / Math.max(8, infillPct));
    for (let x = ib.minX + step; x < ib.maxX - step * 0.4; x += step) {
      segs.push({
        type: "infill",
        points: [
          [round(x), round(ib.minY)],
          [round(x), round(ib.maxY)],
        ],
      });
    }
    for (let y = ib.minY + step; y < ib.maxY - step * 0.4; y += step) {
      segs.push({
        type: "infill",
        points: [
          [round(ib.minX), round(y)],
          [round(ib.maxX), round(y)],
        ],
      });
    }
  }

  if (segs.length) layers.push({ z: round(z), segments: segs });
}

const filament_m = estimateFilament(layers, lineW, layerH);
const time_s = Math.round(32 + layers.length * 0.42 + filament_m * 8);

const modelName = basename(inRel);
const payload = {
  source:
    `2.5D plane-cut preview of ${inRel} — illustrative mesh slice for the web viewer; not a CuraEngine export (CuraEngine build on agent VM failed: CMake submodules / AppImage fetch).`,
  credit:
    modelName.includes("t-wing") || modelName.includes("bridge")
      ? "Model: Shale repo examples/ (AGPL-3.0-or-later)."
      : undefined,
  meta: {
    model: inRel,
    printer: "bambu_lab_a1",
    layer_height_mm: layerH,
    infill_percent: infillPct,
    walls: wallCount,
    layers: layers.length,
    filament_m: round(filament_m),
    time_s,
    bounds,
  },
  plate_mm: 256,
  layers,
};

writeFileSync(outPath, JSON.stringify(payload));
console.log(`Wrote ${outPath}: ${layers.length} layers, footprint ${round(bounds.maxX - bounds.minX)}×${round(bounds.maxY - bounds.minY)} mm`);

function estimateFilament(layers, lineW, lh) {
  let mm = 0;
  for (const lay of layers) {
    for (const seg of lay.segments) {
      for (let i = 1; i < seg.points.length; i++) {
        const [x0, y0] = seg.points[i - 1];
        const [x1, y1] = seg.points[i];
        mm += Math.hypot(x1 - x0, y1 - y0);
      }
    }
  }
  return (mm * lineW * 0.04 * lh) / 1000;
}

function insetBounds(bounds, z, inset) {
  const t = 1 - z / Math.max(bounds.maxZ, 1);
  const cx = (bounds.minX + bounds.maxX) / 2;
  const cy = (bounds.minY + bounds.maxY) / 2;
  const halfX = (bounds.maxX - bounds.minX) / 2 - inset - t * 2;
  const halfY = (bounds.maxY - bounds.minY) / 2 - inset - t * 2;
  if (halfX <= 2 || halfY <= 2) return null;
  return {
    minX: cx - halfX,
    maxX: cx + halfX,
    minY: cy - halfY,
    maxY: cy + halfY,
  };
}

function sliceAtZ(tris, z) {
  const eps = 1e-5;
  const out = [];
  for (const [a, b, c] of tris) {
    const pts = [a, b, c];
    const under = pts.filter((p) => p[2] <= z + eps);
    const over = pts.filter((p) => p[2] > z + eps);
    if (under.length === 0 || over.length === 0) continue;
    const hits = [];
    for (let i = 0; i < 3; i++) {
      const p1 = pts[i];
      const p2 = pts[(i + 1) % 3];
      if (Math.abs(p1[2] - p2[2]) < eps) continue;
      const t = (z - p1[2]) / (p2[2] - p1[2]);
      if (t >= -eps && t <= 1 + eps) {
        hits.push([p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1])]);
      }
    }
    if (hits.length === 2) out.push(hits);
  }
  return out;
}

function chainSegments(segments) {
  const left = segments.map((s) => [
    [s[0][0], s[0][1]],
    [s[1][0], s[1][1]],
  ]);
  const loops = [];
  const used = new Set();

  for (let i = 0; i < left.length; i++) {
    if (used.has(i)) continue;
    let loop = [...left[i]];
    used.add(i);
    let extended = true;
    while (extended) {
      extended = false;
      const end = loop[loop.length - 1];
      for (let j = 0; j < left.length; j++) {
        if (used.has(j)) continue;
        const seg = left[j];
        if (dist(end, seg[0]) < 0.15) {
          loop.push(seg[1]);
          used.add(j);
          extended = true;
          break;
        }
        if (dist(end, seg[1]) < 0.15) {
          loop.push(seg[0]);
          used.add(j);
          extended = true;
          break;
        }
      }
    }
    loops.push(loop);
  }
  return loops;
}

function closeLoop(loop) {
  if (loop.length < 2) return loop;
  const a = loop[0];
  const b = loop[loop.length - 1];
  if (dist(a, b) > 0.2) loop.push(a);
  return loop;
}

function dist(a, b) {
  return Math.hypot(a[0] - b[0], a[1] - b[1]);
}

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