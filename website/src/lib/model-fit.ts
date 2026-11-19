import type { Layer } from "./demo-context";

/** Axis-aligned box in G-code coordinates (millimetres, bed origin). */
export type Box = {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
  minZ: number;
  maxZ: number;
};

/** A vertex in scene axes: X right, Y up (layer height), Z from the bed's Y. */
export type Point = [number, number, number];

const insideBox = (b: Box, pad: number, x: number, y: number) =>
  x >= b.minX - pad && x <= b.maxX + pad && y >= b.minY - pad && y <= b.maxY + pad;

export const boxCentre = (b: Box): [number, number, number] => [
  (b.minX + b.maxX) / 2,
  (b.minY + b.maxY) / 2,
  (b.minZ + b.maxZ) / 2,
];

export const boxSize = (b: Box): [number, number, number] => [
  b.maxX - b.minX,
  b.maxY - b.minY,
  b.maxZ - b.minZ,
];

/**
 * The box the print actually occupies.
 *
 * `meta.bounds` describes the model, but a slice file also carries what the
 * preview never draws: the prime line, the purge blob, the odd stray segment
 * at the plate edge. `demo-slice.json` holds one 636-point "infill" run down
 * x = -176 — measured over raw points it makes the part nine times wider than
 * it is, and a camera fitted to that puts a 40 mm wing in a 350 mm frame.
 *
 * So the box is taken over the segments that survive the same filtering the
 * renderer applies (no travel, no skirt, nothing that leaves the model box),
 * and only their points are measured. If nothing survives the reference box,
 * the reference is dropped and every extruding segment is measured instead,
 * so a file whose `meta.bounds` is wrong still gets a camera that finds it.
 */
export function modelBounds(layers: Layer[], ref?: Box, pad = 8): Box | null {
  const measure = (useRef: boolean) => {
    let minX = Infinity;
    let maxX = -Infinity;
    let minY = Infinity;
    let maxY = -Infinity;
    let minZ = Infinity;
    let maxZ = -Infinity;
    let segments = 0;

    for (const layer of layers) {
      for (const seg of layer.segments) {
        if (seg.type === "travel" || seg.type === "skirt") continue;
        if (seg.points.length < 2) continue;
        if (useRef && ref && !seg.points.every(([x, y]) => insideBox(ref, pad, x, y))) continue;

        for (const [x, y] of seg.points) {
          if (x < minX) minX = x;
          if (x > maxX) maxX = x;
          if (y < minY) minY = y;
          if (y > maxY) maxY = y;
        }
        if (layer.z < minZ) minZ = layer.z;
        if (layer.z > maxZ) maxZ = layer.z;
        segments++;
      }
    }

    if (!segments) return null;

    // A single-layer file is a plane. Give it one layer of height so the camera
    // still has a box it can reason about.
    if (maxZ - minZ < 0.4) {
      const mid = (minZ + maxZ) / 2;
      minZ = mid - 0.2;
      maxZ = mid + 0.2;
    }

    return { minX, maxX, minY, maxY, minZ, maxZ };
  };

  return measure(true) ?? measure(false);
}

/**
 * The same box in scene axes: X right, Y up (layer height), Z from the bed's Y.
 * `centre` is the G-code point the renderer subtracts from every vertex.
 */
export function toSceneBox(b: Box, centre: { x: number; y: number }): Box {
  return {
    minX: b.minX - centre.x,
    maxX: b.maxX - centre.x,
    minY: b.minZ,
    maxY: b.maxZ,
    minZ: b.minY - centre.y,
    maxZ: b.maxY - centre.y,
  };
}

/**
 * Every vertex the renderer would draw, in scene axes.
 *
 * The camera is fitted to these rather than to the box: the T-wing is a T, and
 * most of the box around it is corners where there is no plastic. A box fit
 * left the part at 42 % of the frame while claiming 66 %; the silhouette fit
 * lands where it says it does.
 */
export function modelPoints(
  layers: Layer[],
  centre: { x: number; y: number },
  ref?: Box,
  pad = 8,
): Point[] {
  const collect = (useRef: boolean) => {
    const pts: Point[] = [];
    let segments = 0;

    for (const layer of layers) {
      for (const seg of layer.segments) {
        if (seg.type === "travel" || seg.type === "skirt") continue;
        if (seg.points.length < 2) continue;
        if (useRef && ref && !seg.points.every(([x, y]) => insideBox(ref, pad, x, y))) continue;

        for (const [x, y] of seg.points) pts.push([x - centre.x, layer.z, y - centre.y]);
        segments++;
      }
    }

    return segments ? pts : null;
  };

  return collect(true) ?? collect(false) ?? [];
}

/** The vertices of one layer, in scene axes. */
export function layerPoints(layer: Layer | undefined, centre: { x: number; y: number }): Point[] {
  if (!layer) return [];
  const pts: Point[] = [];
  for (const seg of layer.segments) {
    if (seg.type === "travel" || seg.type === "skirt") continue;
    for (const [x, y] of seg.points) pts.push([x - centre.x, layer.z, y - centre.y]);
  }
  return pts;
}

export type CameraFit = {
  position: [number, number, number];
  target: [number, number, number];
  distance: number;
};

const normalise = (v: [number, number, number]): [number, number, number] => {
  const len = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / len, v[1] / len, v[2] / len];
};

const cross = (
  a: [number, number, number],
  b: [number, number, number],
): [number, number, number] => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];

const dot = (a: [number, number, number], b: [number, number, number]) =>
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

export type FitOptions = {
  /** Vertical field of view, degrees. */
  fov: number;
  /** Canvas width / height. */
  aspect: number;
  /** Share of the tighter axis the part should occupy. */
  fill?: number;
  direction?: [number, number, number];
  /** Height to look at, instead of the middle of the points. */
  focusY?: number;
  /** Never frame less than this many millimetres across. A nearly empty layer
   *  would otherwise pull the camera inside the part. */
  minExtent?: number;
};

/**
 * Where to put a camera so `points` fill `fill` of the tighter of the two axes.
 *
 * Every vertex is projected onto the camera plane and measured there, so the
 * number is the part's own silhouette rather than a box around it: a bounding
 * sphere is wrong for a part whose diagonal is twice its width, and a box is
 * wrong for anything not shaped like a box. `focusY` moves the point the
 * camera looks at without changing the distance, so a layer slider does not
 * zoom in and out.
 */
export function fitToPoints(points: Point[], opts: FitOptions): CameraFit {
  const { fov, aspect, fill = 0.66, direction = [0.424, 0.53, 0.734], focusY, minExtent = 0 } = opts;

  if (!points.length) {
    return { position: [0, 60, 120], target: [0, 0, 0], distance: 140 };
  }

  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  let minZ = Infinity;
  let maxZ = -Infinity;
  for (const [x, y, z] of points) {
    if (x < minX) minX = x;
    if (x > maxX) maxX = x;
    if (y < minY) minY = y;
    if (y > maxY) maxY = y;
    if (z < minZ) minZ = z;
    if (z > maxZ) maxZ = z;
  }

  const dir = normalise(direction);
  const right = normalise(cross([0, 1, 0], dir));
  const up = cross(dir, right);

  const tanV = Math.tan((fov * Math.PI) / 360);
  const tanH = tanV * Math.max(0.2, aspect);
  // The span, in camera-plane units, that fills `fill` of the frame.
  const needX = 2 * tanH * fill;
  const needY = 2 * tanV * fill;

  // The vertices in camera axes, taken once so the fit below is only divisions.
  const n = points.length;
  const px = new Float64Array(n);
  const py = new Float64Array(n);
  const pz = new Float64Array(n);
  let cx = (minX + maxX) / 2;
  let cy = focusY ?? (minY + maxY) / 2;
  let cz = (minZ + maxZ) / 2;

  const project = () => {
    let sx = 0;
    let sy = 0;
    for (let i = 0; i < n; i++) {
      const p: Point = [points[i][0] - cx, points[i][1] - cy, points[i][2] - cz];
      px[i] = dot(p, right);
      py[i] = dot(p, up);
      pz[i] = dot(p, dir);
      sx = Math.max(sx, Math.abs(px[i]));
      sy = Math.max(sy, Math.abs(py[i]));
    }
    return { sx, sy };
  };

  const spans = (distance: number) => {
    let minXp = Infinity;
    let maxXp = -Infinity;
    let minYp = Infinity;
    let maxYp = -Infinity;
    for (let i = 0; i < n; i++) {
      // A vertex `pz` in front of the target is that much closer to the camera.
      const inv = 1 / Math.max(0.05, distance - pz[i]);
      const x = px[i] * inv;
      const y = py[i] * inv;
      if (x < minXp) minXp = x;
      if (x > maxXp) maxXp = x;
      if (y < minYp) minYp = y;
      if (y > maxYp) maxYp = y;
    }
    return { minXp, maxXp, minYp, maxYp };
  };

  // Start from an orthographic guess, then walk the distance in until the
  // *projected span* is the frame. Measuring the span rather than the farthest
  // offset is what makes the number true: a flat layer seen at an angle has one
  // corner much closer to the camera than the other, and a max-offset fit puts
  // that corner at 66 % while the shape as a whole fills half of that.
  const box = project();
  let distance = Math.max(1, (2 * box.sy) / needY, (2 * box.sx) / needX);

  const settle = () => {
    for (let i = 0; i < 12; i++) {
      const s = spans(distance);
      const scale = Math.max(
        (s.maxXp - s.minXp) / needX,
        (s.maxYp - s.minYp) / needY,
      );
      if (!Number.isFinite(scale) || scale <= 0) break;
      if (Math.abs(scale - 1) < 0.004) break;
      distance *= scale;
    }
  };

  // Two rounds: fit the distance, slide the target so the projected box is
  // centred, fit again.
  for (let round = 0; round < 2; round++) {
    settle();
    const s = spans(distance);
    const midX = (s.minXp + s.maxXp) / 2;
    const midY = (s.minYp + s.maxYp) / 2;
    if (Math.abs(midX) < 1e-3 && Math.abs(midY) < 1e-3) break;
    // A shift of the target moves the camera with it, so the projected offset
    // changes by about shift / distance.
    const dx = (right[0] * midX + up[0] * midY) * distance;
    const dy = (right[1] * midX + up[1] * midY) * distance;
    const dz = (right[2] * midX + up[2] * midY) * distance;
    cx += dx;
    cy += dy;
    cz += dz;
    project();
  }
  settle();

  if (minExtent > 0) {
    distance = Math.max(distance, minExtent / needY);
  }

  return {
    position: [cx + dir[0] * distance, cy + dir[1] * distance, cz + dir[2] * distance],
    target: [cx, cy, cz],
    distance,
  };
}

/** `fitToPoints` over the eight corners of a box. */
export function fitToBox(box: Box, opts: FitOptions): CameraFit {
  const pts: Point[] = [];
  for (const x of [box.minX, box.maxX]) {
    for (const y of [box.minY, box.maxY]) {
      for (const z of [box.minZ, box.maxZ]) pts.push([x, y, z]);
    }
  }
  return fitToPoints(pts, opts);
}
