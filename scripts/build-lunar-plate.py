#!/usr/bin/env python3
"""Build lunar.jpg: half-moon on pure #000 outside the disc. Verifies borders."""
from __future__ import annotations

import math
import sys
from pathlib import Path

from PIL import Image

OUT_W, OUT_H = 640, 480
MOON_MAX = 380
LUMA_BLACK = 14


def smoothstep(edge0: float, edge1: float, x: float) -> float:
    if edge0 == edge1:
        return 0.0 if x < edge0 else 1.0
    t = max(0.0, min(1.0, (x - edge0) / (edge1 - edge0)))
    return t * t * (3.0 - 2.0 * t)


def build(src: Path, dest: Path) -> None:
    moon = Image.open(src).convert("L")
    moon.thumbnail((MOON_MAX, MOON_MAX), Image.Resampling.LANCZOS)
    canvas = Image.new("RGB", (OUT_W, OUT_H), (0, 0, 0))
    ox = (OUT_W - moon.width) // 2 + 28
    oy = (OUT_H - moon.height) // 2
    pixels = moon.load()
    out = canvas.load()
    moon_pixels = 0
    mw, mh = moon.width, moon.height
    cx, cy = mw / 2, mh / 2
    for y in range(mh):
        for x in range(mw):
            nx = (x - cx) / max(cx, 1)
            ny = (y - cy) / max(cy, 1)
            if nx * nx + ny * ny > 1.02:
                continue
            lx = x / max(1, mw - 1)
            shade = 1.0 - smoothstep(0.42, 0.88, lx)
            lum = int(pixels[x, y] * shade)
            if lum < LUMA_BLACK:
                continue
            out[ox + x, oy + y] = (lum, lum, lum)
            moon_pixels += 1

    # Force any stray non-black border columns inside the canvas to zero.
    for x in range(OUT_W):
        column = [out[x, y] for y in range(OUT_H)]
        peak = max(channel for cell in column for channel in cell)
        if 0 < peak <= LUMA_BLACK + 4:
            for y in range(OUT_H):
                out[x, y] = (0, 0, 0)

    for y in range(OUT_H):
        for x in range(OUT_W):
            if out[x, y] != (0, 0, 0):
                continue
    for x in range(OUT_W):
        if out[x, 0] != (0, 0, 0) or out[x, OUT_H - 1] != (0, 0, 0):
            raise SystemExit(f"border not black at top/bottom x={x}")
    for y in range(OUT_H):
        if out[0, y] != (0, 0, 0) or out[OUT_W - 1, y] != (0, 0, 0):
            raise SystemExit(f"border not black at sides y={y}")

    canvas.save(dest, quality=92, optimize=True)
    print(f"{dest}: moon pixels={moon_pixels}, size={OUT_W}x{OUT_H}")


def main() -> None:
    root = Path(__file__).resolve().parent.parent
    src = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("/tmp/shale-plates/moon.jpg")
    dest = root / "apps/desktop/src/field/lunar.jpg"
    build(src, dest)


if __name__ == "__main__":
    main()