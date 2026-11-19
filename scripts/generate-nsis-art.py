#!/usr/bin/env python3
"""Generate NSIS header (150x57) and sidebar (164x314) BMPs for Shale.

Requires Pillow: pip install pillow
"""
from __future__ import annotations

import random
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "apps/desktop/src-tauri/windows/nsis"
ICON = ROOT / "apps/desktop/src-tauri/icons/128x128.png"

BG = (6, 7, 11)
INK = (233, 232, 242)
ACCENT = (169, 166, 255)
MUTED = (120, 116, 160)
PANEL = (22, 24, 34)

SEED = 0x5348414C  # "SHAL" — reproducible star field


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


def lerp(a: int, b: int, t: float) -> int:
    return int(a + (b - a) * t)


def base_gradient(w: int, h: int) -> list[tuple[int, int, int]]:
    pixels = []
    for y in range(h):
        t = y / max(h - 1, 1)
        for _ in range(w):
            r = lerp(BG[0], PANEL[0], t * 0.35)
            g = lerp(BG[1], PANEL[1], t * 0.35)
            b = lerp(BG[2], PANEL[2], t * 0.35)
            pixels.append((r, g, b))
    return pixels


def paint_stars(pixels: list[tuple[int, int, int]], w: int, h: int, count: int, rng: random.Random) -> None:
    for _ in range(count):
        x = rng.randrange(w)
        y = rng.randrange(h)
        bright = rng.choice((80, 110, 140, 175))
        pixels[y * w + x] = (bright, bright, min(255, bright + 25))
        if x + 1 < w and rng.random() < 0.25:
            pixels[y * w + x + 1] = (48, 48, 60)


def set_pixel(pixels: list[tuple[int, int, int]], w: int, x: int, y: int, color: tuple[int, int, int]) -> None:
    if 0 <= x < w and 0 <= y < len(pixels) // w:
        pixels[y * w + x] = color


def blit_rgba(
    pixels: list[tuple[int, int, int]],
    w: int,
    h: int,
    img_rgba,
    dst_x: int,
    dst_y: int,
    scale: float,
) -> None:
    from PIL import Image

    if scale != 1.0:
        nw = max(1, int(img_rgba.width * scale))
        nh = max(1, int(img_rgba.height * scale))
        img_rgba = img_rgba.resize((nw, nh), Image.Resampling.LANCZOS)
    for dy in range(img_rgba.height):
        for dx in range(img_rgba.width):
            r, g, b, a = img_rgba.getpixel((dx, dy))
            if a < 8:
                continue
            x, y = dst_x + dx, dst_y + dy
            if x < 0 or y < 0 or x >= w or y >= h:
                continue
            bg = pixels[y * w + x]
            t = a / 255.0
            pixels[y * w + x] = (
                int(bg[0] * (1 - t) + r * t),
                int(bg[1] * (1 - t) + g * t),
                int(bg[2] * (1 - t) + b * t),
            )


def draw_wordmark(pixels: list[tuple[int, int, int]], w: int, x0: int, y0: int, text: str = "Shale") -> None:
    # Simple 5x7 pixel font blocks for legibility in tiny header
    glyph_w, glyph_h, spacing = 6, 8, 2
    chars = {
        "S": (
            "01110",
            "10001",
            "10000",
            "01110",
            "00001",
            "10001",
            "01110",
        ),
        "h": ("00000", "10000", "10000", "11110", "10001", "10001", "10001"),
        "a": ("00000", "01110", "00001", "01111", "10001", "10001", "01111"),
        "l": ("00100", "00100", "00100", "00100", "00100", "00100", "01110"),
        "e": ("00000", "01110", "10001", "11111", "10000", "10001", "01110"),
    }
    cx = x0
    for ch in text:
        pat = chars.get(ch)
        if not pat:
            cx += glyph_w + spacing
            continue
        for row, line in enumerate(pat):
            for col, bit in enumerate(line):
                if bit == "1":
                    set_pixel(pixels, w, cx + col, y0 + row, INK)
        cx += glyph_w + spacing


def accent_band(pixels: list[tuple[int, int, int]], w: int, h: int, y0: int, y1: int) -> None:
    for y in range(y0, min(y1, h)):
        t = (y - y0) / max(y1 - y0, 1)
        col = (
            lerp(PANEL[0], ACCENT[0], 0.25 + 0.15 * t),
            lerp(PANEL[1], ACCENT[1], 0.25 + 0.15 * t),
            lerp(PANEL[2], ACCENT[2], 0.25 + 0.15 * t),
        )
        for x in range(w):
            pixels[y * w + x] = col


def header_image(logo) -> list[tuple[int, int, int]]:
    w, h = 150, 57
    rng = random.Random(SEED)
    pixels = base_gradient(w, h)
    paint_stars(pixels, w, h, 35, rng)
    accent_band(pixels, w, h, 0, 3)
    blit_rgba(pixels, w, h, logo, 6, 8, scale=0.34)
    draw_wordmark(pixels, w, 46, 20)
    for x in range(4, w - 4):
        set_pixel(pixels, w, x, h - 2, ACCENT if x % 11 == 0 else MUTED)
    return pixels


def sidebar_image(logo) -> list[tuple[int, int, int]]:
    w, h = 164, 314
    rng = random.Random(SEED + 1)
    pixels = base_gradient(w, h)
    paint_stars(pixels, w, h, 120, rng)
    accent_band(pixels, w, h, 0, 6)
    # Soft vignette along the right edge (content area is on the left in NSIS)
    for y in range(h):
        for x in range(int(w * 0.72), w):
            t = (x - int(w * 0.72)) / max(w * 0.28, 1)
            bg = pixels[y * w + x]
            pixels[y * w + x] = (int(bg[0] * (1 - t * 0.45)), int(bg[1] * (1 - t * 0.45)), int(bg[2] * (1 - t * 0.35)))
    blit_rgba(pixels, w, h, logo, (w - int(128 * 0.55)) // 2, 78, scale=0.55)
    draw_wordmark(pixels, w, (w - 6 * 5 - 2 * 4) // 2, 215, "Shale")
    for x in range(20, w - 20):
        if (x + 3) % 9 == 0:
            set_pixel(pixels, w, x, 238, MUTED)
    for x in range(24, 100):
        set_pixel(pixels, w, x, 246, (90, 88, 120))
    accent_band(pixels, w, h, h - 8, h)
    return pixels


def main() -> None:
    try:
        from PIL import Image
    except ImportError as exc:
        raise SystemExit("Install Pillow: pip install pillow") from exc

    if not ICON.is_file():
        raise SystemExit(f"Missing icon: {ICON}")
    logo = Image.open(ICON).convert("RGBA")
    write_bmp(OUT / "header.bmp", 150, 57, header_image(logo))
    write_bmp(OUT / "sidebar.bmp", 164, 314, sidebar_image(logo))
    print("wrote", OUT / "header.bmp", OUT / "sidebar.bmp")


if __name__ == "__main__":
    main()