#!/usr/bin/env python3
"""Plot top-layer XY toolpaths coloured by Z from a Shale G-code file."""
from __future__ import annotations

import argparse
import re
import sys


def parse_moves(path: str) -> list[tuple[float, float, float]]:
    z = 0.0
    moves: list[tuple[float, float, float]] = []
    last_layer_z: list[float] = []
    for raw in open(path, encoding="utf-8", errors="replace"):
        line = raw.strip()
        if line.startswith(";LAYER:"):
            if last_layer_z:
                moves = last_layer_z.copy()
            last_layer_z.clear()
            continue
        if not line.startswith("G1") or "E" not in line:
            continue
        xm = re.search(r"X([-\d.]+)", line)
        ym = re.search(r"Y([-\d.]+)", line)
        zm = re.search(r"Z([-\d.]+)", line)
        if zm:
            z = float(zm.group(1))
        if xm and ym:
            last_layer_z.append((float(xm.group(1)), float(ym.group(1)), z))
    if last_layer_z:
        moves = last_layer_z
    return moves


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("gcode")
    parser.add_argument("-o", "--output", required=True)
    parser.add_argument("--title", default="Top layer Z")
    args = parser.parse_args()
    moves = parse_moves(args.gcode)
    if not moves:
        print("no extrusion moves on top layer", file=sys.stderr)
        return 1
    try:
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib not installed", file=sys.stderr)
        return 2
    xs = [m[0] for m in moves]
    ys = [m[1] for m in moves]
    zs = [m[2] for m in moves]
    sc = plt.scatter(xs, ys, c=zs, s=4, cmap="viridis")
    plt.colorbar(sc, label="Z (mm)")
    plt.xlabel("X (mm)")
    plt.ylabel("Y (mm)")
    plt.title(args.title)
    plt.gca().set_aspect("equal", adjustable="box")
    plt.tight_layout()
    plt.savefig(args.output, dpi=120)
    print(args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())