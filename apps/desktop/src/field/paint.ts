/**
 * ASCII field painter.
 *
 * Glyphs are rasterised once into an alpha atlas. Each frame updates only the
 * cells the drift step, the cursor lens, or the comet actually changed, and
 * the shared clock is capped at about 30 fps by the caller.
 */

export const RAMP = " .:-=+*#%@";
export const CELL = 11;
const LENS = 120;
const BG = [6, 7, 11] as const;

export interface FieldPainter {
  paint(time: number, pointerX: number, pointerY: number, comet: boolean): void;
}

export function createField(
  canvas: HTMLCanvasElement,
  image: HTMLImageElement,
): FieldPainter {
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) {
    return { paint() {} };
  }
  const sample = document.createElement("canvas");
  sample.width = image.width;
  sample.height = image.height;
  const source = sample.getContext("2d", { willReadFrequently: true });
  if (!source) return { paint() {} };
  source.drawImage(image, 0, 0);
  const pixels = source.getImageData(0, 0, image.width, image.height).data;
  const iw = image.width;
  const ih = image.height;
  const masks = glyphMasks();

  let width = 0;
  let height = 0;
  let cols = 0;
  let rows = 0;
  let frame: ImageData | null = null;
  let buffer: Uint8ClampedArray | null = null;
  let baseGlyph = new Uint8Array(0);
  let baseR = new Uint8Array(0);
  let baseG = new Uint8Array(0);
  let baseB = new Uint8Array(0);
  let offsetX = new Float32Array(0);
  let offsetY = new Float32Array(0);
  let lastShift = Number.NaN;
  let fitted = { originX: 0, originY: 0, drawW: 1, drawH: 1 };
  const sparks: { x: number; y: number; born: number; glyph: number }[] = [];
  let lastSparkX = Number.NaN;
  let lastSparkY = Number.NaN;

  const ensure = () => {
    const rect = canvas.getBoundingClientRect();
    const nextW = Math.max(1, Math.floor(rect.width));
    const nextH = Math.max(1, Math.floor(rect.height));
    if (nextW === width && nextH === height && buffer) return false;
    width = nextW;
    height = nextH;
    canvas.width = nextW;
    canvas.height = nextH;
    cols = Math.max(1, Math.ceil(width / CELL));
    rows = Math.max(1, Math.ceil(height / CELL));
    const count = cols * rows;
    frame = context.createImageData(nextW, nextH);
    buffer = frame.data;
    baseGlyph = new Uint8Array(count);
    baseR = new Uint8Array(count);
    baseG = new Uint8Array(count);
    baseB = new Uint8Array(count);
    offsetX = new Float32Array(count);
    offsetY = new Float32Array(count);
    const fit = Math.min(width / iw, height / ih) * 0.72;
    fitted = {
      drawW: iw * fit,
      drawH: ih * fit,
      originX: (width - iw * fit) / 2,
      originY: (height - ih * fit) / 2,
    };
    lastShift = Number.NaN;
    fillBackground(buffer, width, height);
    return true;
  };

  const rebuildBase = (shift: number) => {
    const { originX, originY, drawW, drawH } = fitted;
    for (let row = 0; row < rows; row += 1) {
      for (let col = 0; col < cols; col += 1) {
        const index = row * cols + col;
        const cx = col * CELL + CELL / 2;
        const cy = row * CELL + CELL / 2;
        const localX = cx - originX;
        const localY = ((cy - originY + shift) % drawH + drawH) % drawH;
        if (localX < 0 || localX >= drawW || cy < originY || cy >= originY + drawH) {
          baseGlyph[index] = 0;
          continue;
        }
        const sx = Math.min(iw - 1, Math.floor((localX / drawW) * iw));
        const sy = Math.min(ih - 1, Math.floor((localY / drawH) * ih));
        const pixel = (sy * iw + sx) * 4;
        const red = pixels[pixel];
        const green = pixels[pixel + 1];
        const blue = pixels[pixel + 2];
        const luma = Math.pow((0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255, 0.45);
        const step = Math.round(luma * (RAMP.length - 1));
        baseGlyph[index] = step;
        baseR[index] = red;
        baseG[index] = green;
        baseB[index] = blue;
      }
    }
  };

  const paint = (time: number, pointerX: number, pointerY: number, comet: boolean) => {
    const resized = ensure();
    if (!buffer) return;
    const animate = time > 0;
    const stepPx = Math.max(1, fitted.drawH / ih);
    const shift = animate ? Math.floor((time * 0.004) / stepPx) * stepPx : 0;
    const drift = shift !== lastShift || resized;
    if (drift) {
      rebuildBase(shift);
      lastShift = shift;
    }

    let sprung = false;
    const col0 = Math.max(0, Math.floor((pointerX - LENS - 16) / CELL));
    const col1 = Math.min(cols - 1, Math.ceil((pointerX + LENS + 16) / CELL));
    const row0 = Math.max(0, Math.floor((pointerY - LENS - 16) / CELL));
    const row1 = Math.min(rows - 1, Math.ceil((pointerY + LENS + 16) / CELL));

    const touch = (index: number, cx: number, cy: number) => {
      const dx = cx - pointerX;
      const dy = cy - pointerY;
      const dist = Math.hypot(dx, dy);
      const influence = animate && dist < LENS ? (1 - dist / LENS) ** 2 : 0;
      const desiredX = dist > 0 ? (dx / dist) * influence * 14 : 0;
      const desiredY = dist > 0 ? (dy / dist) * influence * 14 : 0;
      const beforeX = offsetX[index];
      const beforeY = offsetY[index];
      if (animate) {
        offsetX[index] += (desiredX - offsetX[index]) * 0.16;
        offsetY[index] += (desiredY - offsetY[index]) * 0.16;
      } else {
        offsetX[index] = 0;
        offsetY[index] = 0;
      }
      if (Math.abs(offsetX[index]) < 0.35) offsetX[index] = 0;
      if (Math.abs(offsetY[index]) < 0.35) offsetY[index] = 0;
      if (Math.abs(offsetX[index] - beforeX) > 0.4 || Math.abs(offsetY[index] - beforeY) > 0.4) {
        sprung = true;
      }
    };

    if (animate && pointerX > -1e5) {
      for (let row = row0; row <= row1; row += 1) {
        for (let col = col0; col <= col1; col += 1) {
          const index = row * cols + col;
          touch(index, col * CELL + CELL / 2, row * CELL + CELL / 2);
        }
      }
    }
    if (animate) {
      for (let index = 0; index < offsetX.length; index += 1) {
        if (offsetX[index] === 0 && offsetY[index] === 0) continue;
        const col = index % cols;
        const row = (index - col) / cols;
        if (pointerX > -1e5 && col >= col0 && col <= col1 && row >= row0 && row <= row1) continue;
        touch(index, col * CELL + CELL / 2, row * CELL + CELL / 2);
      }
    }

    const sparkMarks = sparks.map((spark) => ({ x: spark.x, y: spark.y }));
    if (comet && animate && pointerX > -1e5) {
      const jumped = Math.hypot(pointerX - lastSparkX, pointerY - lastSparkY);
      if (!Number.isFinite(lastSparkX) || jumped > 10) {
        sparks.push({
          x: pointerX,
          y: pointerY,
          born: time,
          glyph: [1, 5, 6][sparks.length % 3],
        });
        if (sparks.length > 12) sparks.shift();
        lastSparkX = pointerX;
        lastSparkY = pointerY;
        sprung = true;
      }
    }
    for (let index = sparks.length - 1; index >= 0; index -= 1) {
      if (animate && time - sparks[index].born >= 700) sparks.splice(index, 1);
    }

    if (!drift && !resized && !sprung && sparks.length === 0 && sparkMarks.length === 0) return;

    let x0 = 0;
    let y0 = 0;
    let x1 = width;
    let y1 = height;
    if (!drift && !resized) {
      x0 = width;
      y0 = height;
      x1 = 0;
      y1 = 0;
      const grow = (x: number, y: number, pad: number) => {
        x0 = Math.min(x0, x - pad);
        y0 = Math.min(y0, y - pad);
        x1 = Math.max(x1, x + pad);
        y1 = Math.max(y1, y + pad);
      };
      if (sprung && pointerX > -1e5) grow(pointerX, pointerY, LENS + 22);
      for (let index = 0; index < offsetX.length; index += 1) {
        if (offsetX[index] === 0 && offsetY[index] === 0) continue;
        const col = index % cols;
        const row = (index - col) / cols;
        grow(col * CELL + CELL / 2 + offsetX[index], row * CELL + CELL / 2 + offsetY[index], CELL);
      }
      for (const spark of sparkMarks) grow(spark.x, spark.y, CELL);
      for (const spark of sparks) grow(spark.x, spark.y, CELL);
      x0 = Math.max(0, Math.floor(x0));
      y0 = Math.max(0, Math.floor(y0));
      x1 = Math.min(width, Math.ceil(x1));
      y1 = Math.min(height, Math.ceil(y1));
      if (x1 <= x0 || y1 <= y0) return;
    }

    clearRect(buffer, width, x0, y0, x1 - x0, y1 - y0);
    const c0 = Math.max(0, Math.floor((x0 - CELL - 16) / CELL));
    const c1 = Math.min(cols - 1, Math.ceil((x1 + 16) / CELL));
    const r0 = Math.max(0, Math.floor((y0 - CELL - 16) / CELL));
    const r1 = Math.min(rows - 1, Math.ceil((y1 + 16) / CELL));
    for (let row = r0; row <= r1; row += 1) {
      for (let col = c0; col <= c1; col += 1) {
        const index = row * cols + col;
        let step = baseGlyph[index];
        if (step <= 0 && offsetX[index] === 0 && offsetY[index] === 0) continue;
        const cx = col * CELL + CELL / 2;
        const cy = row * CELL + CELL / 2;
        const dx = cx - pointerX;
        const dy = cy - pointerY;
        const dist = Math.hypot(dx, dy);
        const influence = animate && dist < LENS ? (1 - dist / LENS) ** 2 : 0;
        step = Math.min(RAMP.length - 1, step + Math.round(influence * 5));
        if (step <= 0) continue;
        const gain = 1 + influence * 1.35;
        let red = Math.min(255, baseR[index] * gain);
        let green = Math.min(255, baseG[index] * gain);
        let blue = Math.min(255, baseB[index] * gain);
        if (red + green + blue < 24) {
          const ink = 0.22 + influence * 0.5;
          red = 233 * ink;
          green = 232 * ink;
          blue = 242 * ink;
        }
        stamp(
          buffer,
          width,
          height,
          masks[step],
          cx + offsetX[index],
          cy + offsetY[index],
          red,
          green,
          blue,
          1,
        );
      }
    }
    for (const spark of sparks) {
      const age = animate ? (time - spark.born) / 700 : 0.2;
      if (age < 0 || age > 1) continue;
      stamp(buffer, width, height, masks[spark.glyph], spark.x, spark.y, 233, 232, 242, 0.55 * (1 - age));
    }

    if (!frame) return;
    if (drift || resized) {
      context.putImageData(frame, 0, 0);
    } else {
      context.putImageData(frame, 0, 0, x0, y0, x1 - x0, y1 - y0);
    }
  };

  return { paint };
}

function glyphMasks(): Uint8Array[] {
  const atlas = document.createElement("canvas");
  atlas.width = CELL * RAMP.length;
  atlas.height = CELL;
  const context = atlas.getContext("2d", { willReadFrequently: true });
  const masks: Uint8Array[] = [];
  if (!context) {
    for (let index = 0; index < RAMP.length; index += 1) masks.push(new Uint8Array(CELL * CELL));
    return masks;
  }
  context.clearRect(0, 0, atlas.width, atlas.height);
  context.fillStyle = "#ffffff";
  context.font = `${CELL}px "Geist Mono", ui-monospace, monospace`;
  context.textAlign = "center";
  context.textBaseline = "middle";
  for (let index = 0; index < RAMP.length; index += 1) {
    context.fillText(RAMP[index], index * CELL + CELL / 2, CELL / 2 + 0.5);
  }
  const data = context.getImageData(0, 0, atlas.width, atlas.height).data;
  for (let index = 0; index < RAMP.length; index += 1) {
    const mask = new Uint8Array(CELL * CELL);
    for (let y = 0; y < CELL; y += 1) {
      for (let x = 0; x < CELL; x += 1) {
        mask[y * CELL + x] = data[((y * atlas.width + index * CELL + x) * 4) + 3];
      }
    }
    masks.push(mask);
  }
  return masks;
}

function fillBackground(buffer: Uint8ClampedArray, width: number, height: number) {
  clearRect(buffer, width, 0, 0, width, height);
}

function clearRect(
  buffer: Uint8ClampedArray,
  width: number,
  x: number,
  y: number,
  w: number,
  h: number,
) {
  const x0 = Math.max(0, x);
  const y0 = Math.max(0, y);
  const x1 = Math.min(width, x + w);
  const y1 = Math.min(buffer.length / 4 / width, y + h);
  for (let row = y0; row < y1; row += 1) {
    let index = (row * width + x0) * 4;
    for (let col = x0; col < x1; col += 1) {
      buffer[index] = BG[0];
      buffer[index + 1] = BG[1];
      buffer[index + 2] = BG[2];
      buffer[index + 3] = 255;
      index += 4;
    }
  }
}

function stamp(
  buffer: Uint8ClampedArray,
  width: number,
  height: number,
  mask: Uint8Array,
  cx: number,
  cy: number,
  red: number,
  green: number,
  blue: number,
  alpha: number,
) {
  const x0 = Math.round(cx - CELL / 2);
  const y0 = Math.round(cy - CELL / 2);
  for (let y = 0; y < CELL; y += 1) {
    const py = y0 + y;
    if (py < 0 || py >= height) continue;
    for (let x = 0; x < CELL; x += 1) {
      const coverage = (mask[y * CELL + x] / 255) * alpha;
      if (coverage < 0.04) continue;
      const px = x0 + x;
      if (px < 0 || px >= width) continue;
      const index = (py * width + px) * 4;
      const keep = 1 - coverage;
      buffer[index] = red * coverage + buffer[index] * keep;
      buffer[index + 1] = green * coverage + buffer[index + 1] * keep;
      buffer[index + 2] = blue * coverage + buffer[index + 2] * keep;
      buffer[index + 3] = 255;
    }
  }
}
