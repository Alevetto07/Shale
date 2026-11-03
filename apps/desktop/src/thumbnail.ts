import type { ModelView } from "./api";
import { rotateScale } from "./coords";

/** Small JPEG of the parts on a near-black field, stored in the project zip. */
export function thumbnailJpeg(models: ModelView[]): number[] {
  const canvas = document.createElement("canvas");
  canvas.width = 320;
  canvas.height = 180;
  const context = canvas.getContext("2d");
  if (!context) return [];
  context.fillStyle = "#06070b";
  context.fillRect(0, 0, 320, 180);
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  const points: [number, number][] = [];
  for (const model of models) {
    const step = Math.max(3, Math.floor(model.positions.length / 6000) * 3);
    for (let index = 0; index < model.positions.length; index += step) {
      const moved = rotateScale(
        [model.positions[index], model.positions[index + 1], model.positions[index + 2]],
        model.transform.rotation_deg,
        model.transform.scale,
      );
      const x = model.transform.translation[0] + moved[0];
      const y = model.transform.translation[1] + moved[1];
      points.push([x, y]);
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    }
  }
  if (points.length > 0) {
    const span = Math.max(maxX - minX, maxY - minY, 1);
    const scale = 150 / span;
    const cx = (minX + maxX) / 2;
    const cy = (minY + maxY) / 2;
    context.fillStyle = "#a39fb4";
    for (const [x, y] of points) {
      context.fillRect(160 + (x - cx) * scale, 90 - (y - cy) * scale, 1.4, 1.4);
    }
  }
  const url = canvas.toDataURL("image/jpeg", 0.72);
  const body = atob(url.split(",")[1] ?? "");
  const bytes = new Array<number>(body.length);
  for (let index = 0; index < body.length; index += 1) bytes[index] = body.charCodeAt(index);
  return bytes;
}
