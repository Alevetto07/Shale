#!/usr/bin/env python3
"""In-layer slope histogram for top-shell G-code (matches Rust metric bins)."""
from __future__ import annotations

import argparse
import json
import math
import re
import sys


def shell_layers(path: str, layer_count: int) -> list[list[tuple[float, float, float]]]:
    layers: list[list[tuple[float, float, float]]] = []
    current: list[tuple[float, float, float]] = []
    x = y = z = 0.0
    for raw in open(path, encoding="utf-8", errors="replace"):
        line = raw.strip()
        if line.startswith(";LAYER:"):
            if current:
                layers.append(current)
            current = []
            continue
        if line.startswith("G1") and "E" in line:
            xm = re.search(r"X([-\d.]+)", line)
            ym = re.search(r"Y([-\d.]+)", line)
            zm = re.search(r"Z([-\d.]+)", line)
            if zm:
                z = float(zm.group(1))
            if xm and ym:
                x = float(xm.group(1))
                y = float(ym.group(1))
                em = re.search(r"E([-\d.]+)", line)
                if em and float(em.group(1)) >= 0:
                    current.append((x, y, z))
    if current:
        layers.append(current)
    return layers[-layer_count:]


def histogram(layers: list[list[tuple[float, float, float]]], bin_width: float) -> list[list[float]]:
    bins: dict[int, int] = {}
    for layer in layers:
        prev = None
        for x, y, z in layer:
            if prev is not None:
                dx = x - prev[0]
                dy = y - prev[1]
                dz = z - prev[2]
                h = math.hypot(dx, dy)
                if h > 1e-6:
                    deg = abs(math.degrees(math.atan2(dz, h)))
                    b = int(deg // bin_width)
                    bins[b] = bins.get(b, 0) + 1
            prev = (x, y, z)
    return [[(b + 0.5) * bin_width, n] for b, n in sorted(bins.items())]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("gcode")
    parser.add_argument("-o", "--output", required=True)
    parser.add_argument("--layers", type=int, default=8)
    parser.add_argument("--bin", type=float, default=0.5)
    parser.add_argument("--title", default="In-layer slope (°)")
    args = parser.parse_args()
    layers = shell_layers(args.gcode, args.layers)
    hist = histogram(layers, args.bin)
    json.dump(hist, open(args.output.replace(".png", ".json") if args.output.endswith(".png") else args.output + ".json", "w"))
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib missing; wrote JSON only", file=sys.stderr)
        return 0
    if not hist:
        return 1
    xs = [h[0] for h in hist]
    ys = [h[1] for h in hist]
    plt.bar(xs, ys, width=args.bin * 0.9)
    plt.xlabel("Slope (degrees)")
    plt.ylabel("Segment count")
    plt.title(args.title)
    plt.tight_layout()
    plt.savefig(args.output, dpi=120)
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())