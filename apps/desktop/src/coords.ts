import type { Transform } from "./api";

/** Bounding-box centre of an untransformed mesh. */
export function localCentre(positions: number[]): [number, number, number] {
  if (positions.length < 3) return [0, 0, 0];
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let index = 0; index < positions.length; index += 3) {
    for (let axis = 0; axis < 3; axis += 1) {
      const value = positions[index + axis];
      min[axis] = Math.min(min[axis], value);
      max[axis] = Math.max(max[axis], value);
    }
  }
  return [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, (min[2] + max[2]) / 2];
}

/** three.js XYZ Euler, matching `mesh::apply_transform` in the core. */
export function rotateScale(
  vertex: [number, number, number],
  rotationDeg: [number, number, number],
  scale: [number, number, number],
): [number, number, number] {
  const x = vertex[0] * scale[0];
  const y = vertex[1] * scale[1];
  const z = vertex[2] * scale[2];
  const rx = (rotationDeg[0] * Math.PI) / 180;
  const ry = (rotationDeg[1] * Math.PI) / 180;
  const rz = (rotationDeg[2] * Math.PI) / 180;
  const a = Math.cos(rx);
  const b = Math.sin(rx);
  const c = Math.cos(ry);
  const d = Math.sin(ry);
  const e = Math.cos(rz);
  const f = Math.sin(rz);
  const te0 = c * e;
  const te4 = -c * f;
  const te8 = d;
  const te1 = a * f + b * e * d;
  const te5 = a * e - b * f * d;
  const te9 = -b * c;
  const te2 = b * f - a * e * d;
  const te6 = b * e + a * f * d;
  const te10 = a * c;
  return [te0 * x + te4 * y + te8 * z, te1 * x + te5 * y + te9 * z, te2 * x + te6 * y + te10 * z];
}

/** World position of the part's centre. This is what the panel and the gizmo show. */
export function worldCentre(positions: number[], transform: Transform): [number, number, number] {
  const offset = rotateScale(localCentre(positions), transform.rotation_deg, transform.scale);
  return [
    transform.translation[0] + offset[0],
    transform.translation[1] + offset[1],
    transform.translation[2] + offset[2],
  ];
}

/** Mesh-origin translation that puts the part centre at `centre`. */
export function translationForCentre(
  positions: number[],
  transform: Transform,
  centre: [number, number, number],
): [number, number, number] {
  const offset = rotateScale(localCentre(positions), transform.rotation_deg, transform.scale);
  return [centre[0] - offset[0], centre[1] - offset[1], centre[2] - offset[2]];
}
