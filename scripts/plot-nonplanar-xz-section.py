#!/usr/bin/env python3
"""Side view (X–Z) of top-shell extrusion moves in G-code."""
from __future__ import annotations

import argparse
import re
import sys


def shell_moves(path: str, layer_count: int) -> list[tuple[float, float, float]]:
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
    shell = layers[-layer_count:] if layers else []
    return [p for layer in shell for p in layer]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("gcode")
    parser.add_argument("-o", "--output", required=True)
    parser.add_argument("--layers", type=int, default=8)
    parser.add_argument("--title", default="Top shell X–Z")
    parser.add_argument("--y-center", type=float, default=None)
    parser.add_argument("--y-tolerance", type=float, default=2.0)
    args = parser.parse_args()
    moves = shell_moves(args.gcode, args.layers)
    if not moves:
        print("no shell moves", file=sys.stderr)
        return 1
    ys = [m[1] for m in moves]
    y_mid = args.y_center if args.y_center is not None else (min(ys) + max(ys)) * 0.5
    subset = [m for m in moves if abs(m[1] - y_mid) <= args.y_tolerance]
    if len(subset) < 10:
        subset = moves
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib not installed", file=sys.stderr)
        return 2
    xs = [m[0] for m in subset]
    zs = [m[2] for m in subset]
    plt.scatter(xs, zs, s=3, c=zs, cmap="viridis")
    plt.xlabel("X (mm)")
    plt.ylabel("Z (mm)")
    plt.title(args.title)
    plt.tight_layout()
    plt.savefig(args.output, dpi=120)
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())