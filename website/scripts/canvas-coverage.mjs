import { PNG } from "pngjs";

/** @param {Buffer} png */
export function coverageFromPng(pngBuffer) {
  const png = PNG.sync.read(pngBuffer);
  const { width: w, height: h, data } = png;
  let minX = w,
    minY = h,
    maxX = 0,
    maxY = 0,
    lit = 0;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (w * y + x) << 2;
      const r = data[i];
      const g = data[i + 1];
      const b = data[i + 2];
      const bright = r + g + b;
      const isBg = r < 18 && g < 16 && b < 20;
      if (!isBg && bright > 60) {
        lit++;
        minX = Math.min(minX, x);
        minY = Math.min(minY, y);
        maxX = Math.max(maxX, x);
        maxY = Math.max(maxY, y);
      }
    }
  }
  if (lit === 0) return { coverage: 0, litPixels: 0 };
  const boxArea = (maxX - minX + 1) * (maxY - minY + 1);
  return { coverage: boxArea / (w * h), litPixels: lit };
}