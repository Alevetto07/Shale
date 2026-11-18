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
PANEL = (22, 24, 34)


def write_bmp(path: Path, width: int, height: int, pixels: list[tuple[int, int, int]]) -> None:
    row_bytes = width * 3
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
    header = struct.pack("<2sIHHI", b"BM", file_size, 0, 0, header_size)
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


def gradient(w: int, h: int) -> list[tuple[int, int, int]]:
    pixels = []
    for y in range(h):
        for x in range(w):
            t = y / max(h - 1, 1)
            r = int(BG[0] * (1 - t) + PANEL[0] * t)
            g = int(BG[1] * (1 - t) + PANEL[1] * t)
            b = int(BG[2] * (1 - t) + PANEL[2] * t)
            pixels.append((r, g, b))
    return pixels


def header_image() -> list[tuple[int, int, int]]:
    w, h = 150, 57
    pixels = gradient(w, h)
    for y in range(h):
        for x in range(w):
            if y < 2 or y >= h - 2:
                pixels[y * w + x] = ACCENT if x % 9 == 0 else (42, 44, 58)
    # left accent bar
    for y in range(4, h - 4):
        for x in range(0, 3):
            pixels[y * w + x] = ACCENT
    # wordmark blocks
    for y in range(20, 38):
        for x in range(16, 96):
            if (x + y) % 7 == 0:
                pixels[y * w + x] = INK
    for y in range(40, 42):
        for x in range(16, 60):
            pixels[y * w + x] = (120, 116, 160)
    return pixels


def sidebar_image() -> list[tuple[int, int, int]]:
    w, h = 164, 314
    pixels = gradient(w, h)
    for x in range(w):
        pixels[x] = ACCENT if x % 12 < 3 else (36, 38, 52)
    cx, cy = w // 2, h // 2 - 18
    for dy in range(-42, 43):
        for dx in range(-42, 43):
            dist = dx * dx + dy * dy
            if dist < 40 * 40:
                xx, yy = cx + dx, cy + dy
                if 0 <= xx < w and 0 <= yy < h:
                    shade = 26 + (dist // 120)
                    pixels[yy * w + xx] = (shade, shade + 2, shade + 8)
    for y in range(cy + 54, cy + 66):
        for x in range(28, w - 28):
            if (x + y) % 5 == 0:
                pixels[y * w + x] = INK
    for y in range(h - 36, h - 8):
        for x in range(12, w - 12):
            if y == h - 36 or y == h - 9:
                pixels[y * w + x] = (48, 50, 66)
    return pixels


def main() -> None:
    write_bmp(OUT / "header.bmp", 150, 57, header_image())
    write_bmp(OUT / "sidebar.bmp", 164, 314, sidebar_image())
    print("wrote", OUT / "header.bmp", OUT / "sidebar.bmp")


if __name__ == "__main__":
    main()