import { useMemo } from "react";
import * as THREE from "three";
import { featureColor } from "../../lib/gcode-types";
import type { Layer } from "../../lib/demo-context";

const MAX_JUMP_MM = 12;

function pathLength(pts: number[][]) {
  let d = 0;
  for (let i = 1; i < pts.length; i++) {
    d += Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]);
  }
  return d;
}

function extrusionRadius(type: string, active: boolean): number {
  if (!active) return 0.08;
  switch (type) {
    case "wall-outer":
      return 0.22;
    case "wall-inner":
      return 0.18;
    case "skin":
      return 0.16;
    case "infill":
      return 0.1;
    case "support":
      return 0.12;
    case "travel":
      return 0.05;
    default:
      return 0.1;
  }
}

function splitChunks(points: number[][]): number[][][] {
  const chunks: number[][][] = [];
  let cur: number[][] = [];
  for (const p of points) {
    if (
      cur.length > 0 &&
      Math.hypot(p[0] - cur[cur.length - 1][0], p[1] - cur[cur.length - 1][1]) > MAX_JUMP_MM
    ) {
      if (cur.length >= 2) chunks.push(cur);
      cur = [];
    }
    cur.push(p);
  }
  if (cur.length >= 2) chunks.push(cur);
  return chunks;
}

function PathTube({
  points,
  z,
  type,
  active,
  center,
}: {
  points: number[][];
  z: number;
  type: string;
  active: boolean;
  center: { x: number; y: number };
}) {
  const mesh = useMemo(() => {
    const chunks = splitChunks(points);
    const chunk = chunks.find((c) => pathLength(c) <= 80) ?? chunks[0];
    if (!chunk || chunk.length < 2 || pathLength(chunk) > 80) return null;

    const vecs = chunk.map(([px, py]) => new THREE.Vector3(px - center.x, z, py - center.y));
    if (vecs.length < 2) return null;

    const curve = new THREE.CatmullRomCurve3(vecs, false, "catmullrom", 0.12);
    const tubularSegments = Math.min(64, vecs.length * 3);
    const geo = new THREE.TubeGeometry(
      curve,
      tubularSegments,
      extrusionRadius(type, active),
      6,
      false,
    );
    const col = new THREE.Color(featureColor(type));
    const mat = new THREE.MeshStandardMaterial({
      color: col,
      emissive: col,
      emissiveIntensity: active ? 0.35 : 0.08,
      transparent: type === "travel",
      opacity: type === "travel" ? 0.25 : active ? 0.92 : 0.22,
      roughness: 0.45,
      metalness: type === "wall-outer" ? 0.15 : 0.05,
    });
    return { geo, mat };
  }, [points, z, type, active, center]);

  if (!mesh) return null;
  return <mesh geometry={mesh.geo} material={mesh.mat} />;
}

type Bbox = { minX: number; maxX: number; minY: number; maxY: number };

function segmentOnModel(seg: { points: number[][] }, bbox: Bbox, pad = 8) {
  return seg.points.every(([x, y]) => {
    return (
      x >= bbox.minX - pad &&
      x <= bbox.maxX + pad &&
      y >= bbox.minY - pad &&
      y <= bbox.maxY + pad
    );
  });
}

export function ExtrusionLayers({
  layers,
  maxLayer,
  center,
  showTravel,
  buildProgress,
  singleLayer = false,
  bbox,
}: {
  layers: Layer[];
  maxLayer: number;
  center: { x: number; y: number };
  showTravel: boolean;
  buildProgress: number;
  singleLayer?: boolean;
  bbox: Bbox;
}) {
  return (
    <group>
      {layers.map((lay, li) => {
        if (li > maxLayer) return null;
        if (singleLayer && li !== maxLayer) return null;
        const active = li === maxLayer;
        return lay.segments.map((seg, si) => {
          if (seg.type === "travel" && !showTravel) return null;
          if (seg.type === "skirt") return null;
          if (!segmentOnModel(seg, bbox)) return null;
          if (seg.points.length < 2) return null;

          let pts = seg.points;
          if (active && buildProgress < 1) {
            const keep = Math.max(2, Math.floor(pts.length * buildProgress));
            pts = pts.slice(0, keep);
          }

          return (
            <PathTube
              key={`${li}-${si}-${seg.type}`}
              points={pts}
              z={lay.z}
              type={seg.type}
              active={active}
              center={center}
            />
          );
        });
      })}
    </group>
  );
}