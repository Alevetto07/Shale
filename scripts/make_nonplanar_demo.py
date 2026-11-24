#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Generate examples/nonplanar-demo.stl: a contoured enclosure lid.

The part is 80 x 50 mm in plan, 16.1 mm tall, with

* a rounded-rectangle plan (10 mm corner radius),
* a 0.8 x 1.6 mm chamfered base edge,
* vertical walls whose top edge follows the lid contour,
* a doubly-curved "pillow" top, ``z = RIM + HX*(1-u^2)^2 + HY*(1-v^2)^2``
  with ``u = x/40`` and ``v = y/25``.

The top is smooth up to the rim (zero gradient on the whole outline), so the
steepest facet in the printed part is well inside 8 degrees and nothing except
the chamfer faces downward.

The mesh is built as one closed surface with shared vertices, so it is
watertight by construction; the script then re-checks that on the float32
coordinates that actually land in the file:

* every directed edge appears exactly once (and its reverse once) -- a
  watertight, consistently wound manifold,
* the signed-tetrahedron volume is positive (outward normals),
* no top facet is steeper than 8 degrees,
* no downward-facing facet above the bed is flatter than 45 degrees from the
  horizontal (the base chamfer is 63.4 degrees from horizontal).

Output is byte-for-byte deterministic: fixed tessellation, fixed header, no
timestamps and no floating-point randomness.  Run ``python scripts/make_nonplanar_demo.py``
to rewrite ``examples/nonplanar-demo.stl``; pass ``-o PATH`` to write elsewhere.
"""

from __future__ import annotations

import argparse
import math
import struct
from collections import Counter
from pathlib import Path

# Plan and rim dimensions, millimetres.
HALF_X = 40.0
HALF_Y = 25.0
CORNER_R = 10.0
RIM_Z = 13.0
BEVEL_INSET = 0.8
BEVEL_Z = 1.6

# Lid contour: each term is zero with zero gradient on its own boundary, so the
# steepest point is in the middle of the lid, not at the rim.
DOME_X = 1.8
DOME_Y = 1.3

# Tessellation: 80 outline points (10 per fillet + 10 per straight side) and 24
# rings from the rim to the centre.  Coarser rings would put facet slope above
# the analytic maximum; finer ones only add file size.
ARC_STEPS = 10
SIDE_STEPS = 10
RINGS = 24

TOP_CAP_DEG = 8.0
OVERHANG_CAP_DEG = 45.0


def lid_z(x: float, y: float) -> float:
    """Height of the contoured top at (x, y) inside the plan."""
    u = x / HALF_X
    v = y / HALF_Y
    return RIM_Z + DOME_X * (1.0 - u * u) ** 2 + DOME_Y * (1.0 - v * v) ** 2


def lid_slope_deg(x: float, y: float) -> float:
    """Angle above the horizontal of the lid surface at (x, y), in degrees."""
    u = x / HALF_X
    v = y / HALF_Y
    dz_dx = DOME_X * (-4.0 * u * (1.0 - u * u)) / HALF_X
    dz_dy = DOME_Y * (-4.0 * v * (1.0 - v * v)) / HALF_Y
    return math.degrees(math.atan(math.hypot(dz_dx, dz_dy)))


def outline(half_x: float, half_y: float, radius: float) -> list[tuple[float, float]]:
    """Rounded-rectangle perimeter, counter-clockwise seen from +Z.

    The straight-run and fillet counts are fixed, so calling this with smaller
    half extents (the base chamfer inset) yields the same number of points in
    the same order: point ``i`` of one outline lies on the same ray as point
    ``i`` of the other.
    """
    if radius <= 0.0 or radius > min(half_x, half_y):
        raise ValueError("corner radius must be inside the plan")

    cx = half_x - radius
    cy = half_y - radius
    points: list[tuple[float, float]] = []

    def edge(x0: float, y0: float, x1: float, y1: float) -> None:
        for step in range(SIDE_STEPS):
            t = step / SIDE_STEPS
            points.append((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t))

    def arc(ccx: float, ccy: float, start_deg: float, end_deg: float) -> None:
        for step in range(ARC_STEPS):
            t = step / ARC_STEPS
            angle = math.radians(start_deg + (end_deg - start_deg) * t)
            points.append((ccx + radius * math.cos(angle), ccy + radius * math.sin(angle)))

    # Bottom edge left to right, then round the plan counter-clockwise.
    edge(-cx, -half_y, cx, -half_y)
    arc(cx, -cy, -90.0, 0.0)
    edge(half_x, -cy, half_x, cy)
    arc(cx, cy, 0.0, 90.0)
    edge(cx, half_y, -cx, half_y)
    arc(-cx, cy, 90.0, 180.0)
    edge(-half_x, cy, -half_x, -cy)
    arc(-cx, -cy, 180.0, 270.0)
    return points


class Mesh:
    def __init__(self) -> None:
        self.vertices: list[tuple[float, float, float]] = []
        self.faces: list[tuple[int, int, int]] = []

    def vertex(self, x: float, y: float, z: float) -> int:
        self.vertices.append((x, y, z))
        return len(self.vertices) - 1

    def face(self, a: int, b: int, c: int) -> None:
        self.faces.append((a, b, c))


def build_mesh() -> Mesh:
    mesh = Mesh()
    plan = outline(HALF_X, HALF_Y, CORNER_R)
    base_plan = outline(HALF_X - BEVEL_INSET, HALF_Y - BEVEL_INSET, CORNER_R - BEVEL_INSET)
    count = len(plan)

    # Rings: ring 0 is the rim, the last ring is the smallest scaled copy.
    rings: list[list[int]] = []
    for k in range(RINGS):
        scale = 1.0 - k / RINGS
        ring = []
        for x, y in plan:
            sx = x * scale
            sy = y * scale
            ring.append(mesh.vertex(sx, sy, lid_z(sx, sy)))
        rings.append(ring)
    top_centre = mesh.vertex(0.0, 0.0, lid_z(0.0, 0.0))

    # The top surface: quads between neighbouring rings, then a fan to the centre.
    for k in range(RINGS - 1):
        outer, inner = rings[k], rings[k + 1]
        for i in range(count):
            j = (i + 1) % count
            mesh.face(outer[i], outer[j], inner[j])
            mesh.face(outer[i], inner[j], inner[i])
    last = rings[-1]
    for i in range(count):
        j = (i + 1) % count
        mesh.face(top_centre, last[i], last[j])

    # Vertical wall, from the chamfer top edge up to the rim ring.
    wall = [mesh.vertex(x, y, BEVEL_Z) for (x, y) in plan]
    for i in range(count):
        j = (i + 1) % count
        mesh.face(wall[i], wall[j], rings[0][j])
        mesh.face(wall[i], rings[0][j], rings[0][i])

    # Chamfered base edge, then the flat bed face.
    base = [mesh.vertex(x, y, 0.0) for (x, y) in base_plan]
    for i in range(count):
        j = (i + 1) % count
        mesh.face(base[i], base[j], wall[j])
        mesh.face(base[i], wall[j], wall[i])
    bottom_centre = mesh.vertex(0.0, 0.0, 0.0)
    for i in range(count):
        j = (i + 1) % count
        mesh.face(bottom_centre, base[j], base[i])

    return mesh


def as_float32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def quantise(mesh: Mesh) -> Mesh:
    """Return the mesh as the STL will store it: every coordinate in float32."""
    snapped = Mesh()
    for x, y, z in mesh.vertices:
        snapped.vertices.append((as_float32(x), as_float32(y), as_float32(z)))
    snapped.faces = list(mesh.faces)
    return snapped


def face_normal(mesh: Mesh, face: tuple[int, int, int]) -> tuple[float, float, float]:
    ax, ay, az = mesh.vertices[face[0]]
    bx, by, bz = mesh.vertices[face[1]]
    cx, cy, cz = mesh.vertices[face[2]]
    ux, uy, uz = bx - ax, by - ay, bz - az
    vx, vy, vz = cx - ax, cy - ay, cz - az
    nx = uy * vz - uz * vy
    ny = uz * vx - ux * vz
    nz = ux * vy - uy * vx
    length = math.sqrt(nx * nx + ny * ny + nz * nz)
    if length == 0.0:
        raise SystemExit(f"degenerate face {face}")
    return (nx / length, ny / length, nz / length)


def signed_volume(mesh: Mesh) -> float:
    total = 0.0
    for a, b, c in mesh.faces:
        ax, ay, az = mesh.vertices[a]
        bx, by, bz = mesh.vertices[b]
        cx, cy, cz = mesh.vertices[c]
        total += (
            ax * (by * cz - bz * cy)
            - ay * (bx * cz - bz * cx)
            + az * (bx * cy - by * cx)
        )
    return total / 6.0


def check(mesh: Mesh) -> dict[str, float]:
    """Watertightness, winding, slope and overhang assertions plus the numbers."""
    directed = Counter()
    for a, b, c in mesh.faces:
        for start, end in ((a, b), (b, c), (c, a)):
            directed[(start, end)] += 1
    repeated = [edge for edge, uses in directed.items() if uses != 1]
    if repeated:
        raise SystemExit(f"edge(s) used more than once in one direction: {repeated[:5]}")
    for (start, end), uses in directed.items():
        if directed.get((end, start), 0) != 1 or uses != 1:
            raise SystemExit(f"edge {start}->{end} has no matching reverse: not a closed manifold")

    volume = signed_volume(mesh)
    if volume <= 0.0:
        raise SystemExit(f"signed volume {volume:.3f} mm3: faces are wound inward")

    worst_top = 0.0
    worst_top_face = None
    worst_overhang = 0.0
    worst_overhang_face = None
    for face in mesh.faces:
        nx, ny, nz = face_normal(mesh, face)
        if nz > 1e-9:
            slope = math.degrees(math.atan(math.hypot(nx, ny) / nz))
            if slope > worst_top:
                worst_top, worst_top_face = slope, face
        elif nz < -1e-9:
            # Faces lying on the bed are supported by the bed, not overhangs.
            if all(mesh.vertices[index][2] <= 1e-6 for index in face):
                continue
            overhang = math.degrees(math.asin(min(1.0, -nz)))
            if overhang > worst_overhang:
                worst_overhang, worst_overhang_face = overhang, face

    if worst_top > TOP_CAP_DEG + 1e-9:
        raise SystemExit(
            f"top facet at {worst_top:.2f} deg exceeds {TOP_CAP_DEG:.1f} deg: {worst_top_face}"
        )
    if worst_overhang > OVERHANG_CAP_DEG + 1e-9:
        raise SystemExit(
            f"overhang facet at {worst_overhang:.2f} deg exceeds "
            f"{OVERHANG_CAP_DEG:.1f} deg: {worst_overhang_face}"
        )

    xs = [v[0] for v in mesh.vertices]
    ys = [v[1] for v in mesh.vertices]
    zs = [v[2] for v in mesh.vertices]
    return {
        "triangles": len(mesh.faces),
        "vertices": len(mesh.vertices),
        "volume_mm3": volume,
        "size_x": max(xs) - min(xs),
        "size_y": max(ys) - min(ys),
        "size_z": max(zs) - min(zs),
        "worst_top_facet_deg": worst_top,
        "worst_overhang_deg": worst_overhang,
    }


def analytic_max_slope_deg(samples_x: int = 401, samples_y: int = 251) -> float:
    """Dense sweep of the analytic lid slope over the whole plan.

    Uses the rectangle that contains the rounded plan; outside the part the
    formula is still finite and never steeper than at the centre line, so the
    maximum is not exaggerated.
    """
    worst = 0.0
    for ix in range(samples_x):
        x = -HALF_X + 2.0 * HALF_X * ix / (samples_x - 1)
        for iy in range(samples_y):
            y = -HALF_Y + 2.0 * HALF_Y * iy / (samples_y - 1)
            worst = max(worst, lid_slope_deg(x, y))
    return worst


def write_binary_stl(mesh: Mesh, path: Path) -> int:
    header = b"Shale non-planar demo: contoured enclosure lid 80x50x16 mm".ljust(80, b" ")
    body = bytearray()
    for face in mesh.faces:
        nx, ny, nz = face_normal(mesh, face)
        body += struct.pack("<3f", nx, ny, nz)
        for index in face:
            body += struct.pack("<3f", *mesh.vertices[index])
        body += struct.pack("<H", 0)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as handle:
        handle.write(header)
        handle.write(struct.pack("<I", len(mesh.faces)))
        handle.write(bytes(body))
    return 84 + 50 * len(mesh.faces)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    default_out = Path(__file__).resolve().parents[1] / "examples" / "nonplanar-demo.stl"
    parser.add_argument("-o", "--out", type=Path, default=default_out)
    args = parser.parse_args()

    mesh = quantise(build_mesh())
    stats = check(mesh)
    analytic = analytic_max_slope_deg()
    if analytic > TOP_CAP_DEG + 1e-9:
        raise SystemExit(f"analytic lid slope {analytic:.2f} deg exceeds {TOP_CAP_DEG:.1f} deg")

    size = write_binary_stl(mesh, args.out)

    print(f"part          {args.out}")
    print(f"triangles     {stats['triangles']}")
    print(f"vertices      {stats['vertices']}")
    print(
        f"size          {stats['size_x']:.2f} x {stats['size_y']:.2f} x "
        f"{stats['size_z']:.2f} mm"
    )
    print(f"volume        {stats['volume_mm3']:.1f} mm3 (signed tetrahedra)")
    print(f"top slope     analytic max {analytic:.2f} deg, worst facet {stats['worst_top_facet_deg']:.2f} deg (cap {TOP_CAP_DEG:.0f})")
    print(f"overhang      worst {stats['worst_overhang_deg']:.2f} deg from vertical (cap {OVERHANG_CAP_DEG:.0f})")
    print(f"watertight    yes: every directed edge used once, positive volume")
    print(f"file          {size} bytes")


if __name__ == "__main__":
    main()
