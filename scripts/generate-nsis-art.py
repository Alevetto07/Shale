#!/usr/bin/env python3
"""Generate NSIS header and sidebar BMPs for Shale (24-bit BMP, no compression)."""
from __future__ import annotations

import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "apps/desktop/src-tauri/windows/nsis"
BG = (6, 7, 11)
INK = (233, 232, 242)
ACCENT = (185, 179, 255)


def write_bmp(path: Path, width: int, height: int, pixels: list[tuple[int, int, int]]) -> None:
    row_bytes = width * 3
  # BMP rows are padded to 4 bytes
    stride = (row_bytes + 3) & ~3
    pad = stride - row_bytes
    pixel_bytes = bytearray()
    for y in range(height - 1, -1, -1):
        for x in range(width):
            r, g, b = pixels[y * width + x]
            pixel_bytes.extend([b, g, r])
        pixel_bytes.extend(b"\x00" * pad)
    header_size = 14 + 40
    data_size = len(pixel_bytes)
    file_size = header_size + data_size
    header = struct.pack(
        "<2sIHHI",
        b"BM",
        file_size,
        0,
        0,
        header_size,
    )
    dib = struct.pack(
        "<IiiHHIIIIII",
        40,
        width,
        height,
        1,
        24,
        0,
        data_size,
        0,
        0,
        0,
        0,
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(header + dib + pixel_bytes)


def star_field(w: int, h: int, density: int = 400) -> list[tuple[int, int, int]]:
    pixels = [BG] * (w * h)
    seed = 0x5A1E
    for _ in range(density):
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        x = seed % w
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        y = seed % h
        bright = INK if seed % 7 == 0 else (90, 88, 110)
        pixels[y * w + x] = bright
    return pixels


def header_image() -> list[tuple[int, int, int]]:
    w, h = 150, 57
    pixels = star_field(w, h, 120)
    for y in range(h):
        for x in range(w):
            if x < 3 or y < 3 or x >= w - 3 or y >= h - 3:
                pixels[y * w + x] = ACCENT if (x + y) % 6 == 0 else (40, 42, 58)
    for y in range(18, 42):
        for x in range(14, 70):
            pixels[y * w + x] = (28, 30, 42)
    # wordmark blocks (Space Grotesk stand-in)
    for y in range(22, 38):
        for x in range(18, 110):
            if (x + y) % 5 == 0:
                pixels[y * w + x] = INK
    return pixels


def sidebar_image() -> list[tuple[int, int, int]]:
    w, h = 164, 314
    pixels = star_field(w, h, 900)
    for x in range(w):
        pixels[x] = ACCENT if x % 8 < 2 else (32, 34, 48)
    for y in range(h):
        for x in range(w):
            if x < 2:
                pixels[y * w + x] = ACCENT
    # centre mark
    cx, cy = w // 2, h // 2 - 20
    for dy in range(-36, 37):
        for dx in range(-36, 37):
            if dx * dx + dy * dy < 32 * 32:
                xx, yy = cx + dx, cy + dy
                if 0 <= xx < w and 0 <= yy < h:
                    pixels[yy * w + xx] = (28, 30, 44) if (dx + dy) % 6 else (38, 40, 56)
    for y in range(cy + 52, cy + 72):
        for x in range(24, w - 24):
            if (x + y) % 4 == 0:
                pixels[y * w + x] = INK
    return pixels


def main() -> None:
    write_bmp(OUT / "header.bmp", 150, 57, header_image())
    write_bmp(OUT / "sidebar.bmp", 164, 314, sidebar_image())
    print("wrote", OUT / "header.bmp", OUT / "sidebar.bmp")


if __name__ == "__main__":
    main()