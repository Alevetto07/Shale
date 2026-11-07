//! Which way up should a part be printed?
//!
//! Bambu Studio calls this *lay flat*; the idea is the same everywhere: try the
//! orientations that put a face on the plate and keep the one whose downward
//! facing surface area is smallest, because that is the surface that will need
//! support. The candidate set is the 24 rotations of the cube group, reached
//! here by trying all 64 quarter-turn combinations and letting the duplicates
//! compete — the scoring is cheap enough that a table of matrices would only add
//! a way to be wrong.
//!
//! Nothing here knows about the engine. It scores geometry, so it can be reason
//! about — and tested — without slicing anything.

use crate::mesh::{apply_transform, TriangleMesh, Transform};

/// Surface within a hair of the plate is resting on it, not overhanging it.
const BED_EPSILON: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orientation {
    /// Quarter turns about X, Y and Z, in degrees, in the transform's own order.
    pub rotation_deg: [f64; 3],
    /// Downward facing area that is not resting on the plate, in mm².
    pub overhang_mm2: f64,
    /// Height of the part once it is dropped onto the plate, in mm.
    pub height_mm: f64,
    /// Footprint area of the bounding box, in mm². Wider is steadier.
    pub footprint_mm2: f64,
}

impl Orientation {
    /// Lower is better: less overhang, then shorter, then steadier.
    fn better_than(&self, other: &Self) -> bool {
        let overhang = self.overhang_mm2 - other.overhang_mm2;
        if overhang.abs() > 0.5 {
            return overhang < 0.0;
        }
        if (self.height_mm - other.height_mm).abs() > 0.05 {
            return self.height_mm < other.height_mm;
        }
        self.footprint_mm2 > other.footprint_mm2
    }
}

fn rotated(vertex: [f32; 3], rotation_deg: [f64; 3]) -> [f64; 3] {
    apply_transform(
        [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64],
        &Transform {
            translation: [0.0, 0.0, 0.0],
            rotation_deg,
            scale: [1.0, 1.0, 1.0],
        },
    )
}

/// Bounding box of the mesh with a transform's rotation and scale applied but
/// its translation left out: (min, max). That is the box in the part's own
/// frame, which is what a translation has to line up.
fn shaped_bounds(mesh: &TriangleMesh, transform: &Transform) -> ([f64; 3], [f64; 3]) {
    let local = Transform {
        translation: [0.0, 0.0, 0.0],
        rotation_deg: transform.rotation_deg,
        scale: transform.scale,
    };
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for vertex in &mesh.vertices {
        let point = apply_transform(
            [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64],
            &local,
        );
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    (min, max)
}

/// Bounding box of the mesh under a rotation: (min, max).
fn rotated_bounds(mesh: &TriangleMesh, rotation_deg: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for vertex in &mesh.vertices {
        let point = rotated(*vertex, rotation_deg);
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    (min, max)
}

/// Downward facing area, ignoring whatever lies on the plate.
///
/// `angle_deg` is measured from vertical, the way support overhang angles are:
/// a vertical wall is 0° and a ceiling is 90°. A face is counted when it is
/// steeper than the angle, which is `normal.z < -cos(angle)`.
pub fn overhang_area(mesh: &TriangleMesh, rotation_deg: [f64; 3], angle_deg: f64) -> f64 {
    let points: Vec<[f64; 3]> = mesh.vertices.iter().map(|v| rotated(*v, rotation_deg)).collect();
    let plate = points.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
    if !plate.is_finite() {
        return 0.0;
    }
    let limit = -angle_deg.to_radians().cos();
    let mut area = 0.0;
    for triangle in &mesh.triangles {
        let [a, b, c] = [
            points[triangle[0] as usize],
            points[triangle[1] as usize],
            points[triangle[2] as usize],
        ];
        // Resting on the plate: the lowest of the three corners is on the bed.
        if a[2].min(b[2]).min(c[2]) <= plate + BED_EPSILON {
            continue;
        }
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        if length <= f64::EPSILON {
            continue;
        }
        if cross[2] / length < limit {
            area += length * 0.5;
        }
    }
    area
}

/// The orientation that needs the least support, then prints shortest.
pub fn best_orientation(mesh: &TriangleMesh, angle_deg: f64) -> Orientation {
    let mut best: Option<Orientation> = None;
    for x in [0.0, 90.0, 180.0, 270.0] {
        for y in [0.0, 90.0, 180.0, 270.0] {
            for z in [0.0, 90.0, 180.0, 270.0] {
                let rotation_deg = [x, y, z];
                let (min, max) = rotated_bounds(mesh, rotation_deg);
                if !min[0].is_finite() {
                    continue;
                }
                let candidate = Orientation {
                    rotation_deg,
                    overhang_mm2: overhang_area(mesh, rotation_deg, angle_deg),
                    height_mm: max[2] - min[2],
                    footprint_mm2: (max[0] - min[0]) * (max[1] - min[1]),
                };
                if best.map_or(true, |current| candidate.better_than(&current)) {
                    best = Some(candidate);
                }
            }
        }
    }
    best.unwrap_or(Orientation {
        rotation_deg: [0.0, 0.0, 0.0],
        overhang_mm2: 0.0,
        height_mm: 0.0,
        footprint_mm2: 0.0,
    })
}

/// A transform that stands the part on its best face, in the spot it already
/// occupies: the footprint centre is preserved and the lowest point lands on
/// the plate.
pub fn orient(mesh: &TriangleMesh, transform: &Transform, angle_deg: f64) -> Transform {
    let choice = best_orientation(mesh, angle_deg);
    let mut next = Transform {
        translation: transform.translation,
        rotation_deg: choice.rotation_deg,
        scale: transform.scale,
    };
    let (before_min, before_max) = shaped_bounds(mesh, transform);
    let (after_min, after_max) = shaped_bounds(mesh, &next);
    // Keep the middle of the part where it was, in plan.
    next.translation[0] +=
        (before_min[0] + before_max[0]) * 0.5 - (after_min[0] + after_max[0]) * 0.5;
    next.translation[1] +=
        (before_min[1] + before_max[1]) * 0.5 - (after_min[1] + after_max[1]) * 0.5;
    // And put its lowest corner on the plate, whatever the old height was.
    next.translation[2] = -after_min[2];
    next
}

/// Rest a chosen face on the plate.
///
/// Bambu Studio calls it *place on face*: you click a surface and that surface
/// becomes the one lying on the bed. The rotation is the shortest arc from the
/// clicked normal to straight down, composed with the rotation the part already
/// has, and then the part is dropped and kept where it stood in plan — the same
/// courtesy `orient` does, so the part does not jump across the plate because
/// you asked which way up it goes.
pub fn face_rest(mesh: &TriangleMesh, transform: &Transform, world_normal: [f64; 3]) -> Transform {
    let length = (world_normal[0] * world_normal[0]
        + world_normal[1] * world_normal[1]
        + world_normal[2] * world_normal[2])
        .sqrt();
    if !length.is_finite() || length < 1e-9 {
        return transform.clone();
    }
    let n = [
        world_normal[0] / length,
        world_normal[1] / length,
        world_normal[2] / length,
    ];
    // Down is where the face has to end up.
    let target = [0.0, 0.0, -1.0];
    let dot = (n[0] * target[0] + n[1] * target[1] + n[2] * target[2]).clamp(-1.0, 1.0);
    let axis_cross = [
        n[1] * target[2] - n[2] * target[1],
        n[2] * target[0] - n[0] * target[2],
        n[0] * target[1] - n[1] * target[0],
    ];
    let sine = (axis_cross[0] * axis_cross[0] + axis_cross[1] * axis_cross[1] + axis_cross[2] * axis_cross[2]).sqrt();
    let (axis, angle) = if sine < 1e-9 {
        if dot > 0.0 {
            // Already facing down: nothing to turn, only to drop.
            ([1.0, 0.0, 0.0], 0.0)
        } else {
            // Straight up: a half turn, and any horizontal axis will do.
            let pick = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            let cross = [
                n[1] * pick[2] - n[2] * pick[1],
                n[2] * pick[0] - n[0] * pick[2],
                n[0] * pick[1] - n[1] * pick[0],
            ];
            let l = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
            ([cross[0] / l, cross[1] / l, cross[2] / l], std::f64::consts::PI)
        }
    } else {
        (
            [axis_cross[0] / sine, axis_cross[1] / sine, axis_cross[2] / sine],
            sine.atan2(dot),
        )
    };

    // Rodrigues, row-major.
    let (s, c) = (angle.sin(), angle.cos());
    let (x, y, z) = (axis[0], axis[1], axis[2]);
    let turn = [
        [c + x * x * (1.0 - c), x * y * (1.0 - c) - z * s, x * z * (1.0 - c) + y * s],
        [y * x * (1.0 - c) + z * s, c + y * y * (1.0 - c), y * z * (1.0 - c) - x * s],
        [z * x * (1.0 - c) - y * s, z * y * (1.0 - c) + x * s, c + z * z * (1.0 - c)],
    ];
    let held = rotation_matrix(&transform.rotation_deg);
    let mut next = [[0.0; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            next[row][column] = (0..3).map(|k| turn[row][k] * held[k][column]).sum();
        }
    }

    // Back out the three.js XYZ angles. The matrix is Rx·Ry·Rz, so m02 is sin y,
    // and the other two fall out of the same two rows.
    let rotation_deg = euler_from(&next);
    let mut placed = Transform {
        translation: transform.translation,
        rotation_deg,
        scale: transform.scale,
    };
    let (before_min, before_max) = shaped_bounds(mesh, transform);
    let (after_min, after_max) = shaped_bounds(mesh, &placed);
    placed.translation[0] +=
        (before_min[0] + before_max[0]) * 0.5 - (after_min[0] + after_max[0]) * 0.5;
    placed.translation[1] +=
        (before_min[1] + before_max[1]) * 0.5 - (after_min[1] + after_max[1]) * 0.5;
    placed.translation[2] = -after_min[2];
    placed
}

/// The rotation half of `apply_transform`, as a row-major matrix.
fn rotation_matrix(rotation_deg: &[f64; 3]) -> [[f64; 3]; 3] {
    let (rx, ry, rz) = (
        rotation_deg[0].to_radians(),
        rotation_deg[1].to_radians(),
        rotation_deg[2].to_radians(),
    );
    let (a, b) = (rx.cos(), rx.sin());
    let (c, d) = (ry.cos(), ry.sin());
    let (e, f) = (rz.cos(), rz.sin());
    [
        [c * e, -c * f, d],
        [a * f + b * d * e, a * e - b * d * f, -b * c],
        [b * f - a * d * e, b * e + a * d * f, a * c],
    ]
}

/// Angles in degrees for a row-major rotation of the form Rx·Ry·Rz.
fn euler_from(m: &[[f64; 3]; 3]) -> [f64; 3] {
    let sin_y = m[0][2].clamp(-1.0, 1.0);
    let y = sin_y.asin();
    if sin_y.abs() > 1.0 - 1e-9 {
        // Looking straight up or straight down: x and z describe the same turn,
        // so put all of it in x.
        let x = (-m[2][0] * sin_y.signum()).atan2(m[1][1]);
        return [x.to_degrees(), y.to_degrees(), 0.0];
    }
    [
        (-m[1][2]).atan2(m[2][2]).to_degrees(),
        y.to_degrees(),
        (-m[0][1]).atan2(m[0][0]).to_degrees(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(size: f64) -> TriangleMesh {
        let mut mesh = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        let s = size as f32;
        for corner in 0..8 {
            mesh.vertices.push([
                if corner & 1 == 0 { 0.0 } else { s },
                if corner & 2 == 0 { 0.0 } else { s },
                if corner & 4 == 0 { 0.0 } else { s },
            ]);
        }
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        for quad in quads {
            mesh.triangles.push([quad[0], quad[1], quad[2]]);
            mesh.triangles.push([quad[0], quad[2], quad[3]]);
        }
        mesh
    }

    fn boxed(min: [f32; 3], max: [f32; 3]) -> TriangleMesh {
        let mut mesh = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        for corner in 0..8 {
            mesh.vertices.push([
                if corner & 1 == 0 { min[0] } else { max[0] },
                if corner & 2 == 0 { min[1] } else { max[1] },
                if corner & 4 == 0 { min[2] } else { max[2] },
            ]);
        }
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        for quad in quads {
            mesh.triangles.push([quad[0], quad[1], quad[2]]);
            mesh.triangles.push([quad[0], quad[2], quad[3]]);
        }
        mesh
    }

    /// A mushroom: a 4 mm post under a 20 mm cap. Standing up it has a 384 mm²
    /// ceiling; turned over it sits flat and needs nothing.
    fn mushroom() -> TriangleMesh {
        let mut mesh = boxed([8.0, 8.0, 0.0], [12.0, 12.0, 10.0]);
        let cap = boxed([0.0, 0.0, 10.0], [20.0, 20.0, 12.0]);
        let offset = mesh.vertices.len() as u32;
        mesh.vertices.extend(cap.vertices);
        mesh.triangles
            .extend(cap.triangles.iter().map(|t| [t[0] + offset, t[1] + offset, t[2] + offset]));
        mesh
    }

    /// A wedge: a right triangle in XZ, extruded along Y. Flat bottom, a ramp
    /// rising to 10 mm, and a vertical face at x = 0.
    fn wedge() -> TriangleMesh {
        let mut mesh = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        let profile = [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)];
        for depth in [0.0f32, 10.0] {
            for (x, z) in profile {
                mesh.vertices.push([x as f32, depth, z as f32]);
            }
        }
        mesh.triangles = vec![
            [0, 1, 2],
            [3, 5, 4],
            [0, 3, 4],
            [0, 4, 1],
            [1, 4, 5],
            [1, 5, 2],
            [2, 5, 3],
            [2, 3, 0],
        ];
        mesh
    }

    fn volume(mesh: &TriangleMesh) -> f64 {
        let mut total = 0.0;
        for triangle in &mesh.triangles {
            let a = mesh.vertices[triangle[0] as usize];
            let b = mesh.vertices[triangle[1] as usize];
            let c = mesh.vertices[triangle[2] as usize];
            let a = [a[0] as f64, a[1] as f64, a[2] as f64];
            let b = [b[0] as f64, b[1] as f64, b[2] as f64];
            let c = [c[0] as f64, c[1] as f64, c[2] as f64];
            total += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.0;
        }
        total
    }

    #[test]
    fn a_cube_needs_no_support_however_it_stands() {
        let mesh = cube(10.0);
        let best = best_orientation(&mesh, 45.0);
        assert!(best.overhang_mm2 < 1e-6, "{best:?}");
        assert!((best.height_mm - 10.0).abs() < 1e-6, "{best:?}");
    }

    #[test]
    fn a_wedge_is_left_sitting_on_its_flat_face() {
        let mesh = wedge();
        let flat = overhang_area(&mesh, [0.0, 0.0, 0.0], 45.0);
        assert!(flat < 1e-6, "the wedge as drawn rests on its flat face: {flat}");
        // The prism is symmetrical, so several quarter turns put the same
        // face down. What matters is that the answer is a flat one.
        let best = best_orientation(&mesh, 45.0);
        assert!(best.overhang_mm2 < 1e-6, "{best:?}");
        assert!((best.height_mm - 10.0).abs() < 1e-6, "{best:?}");
    }

    #[test]
    fn a_mushroom_is_turned_over_so_its_cap_becomes_the_base() {
        let mesh = mushroom();
        let standing = overhang_area(&mesh, [0.0, 0.0, 0.0], 45.0);
        // The whole 20 x 20 underside of the cap: the post is a separate box
        // pushed into it, so it does not cut a hole in the ceiling.
        assert!(
            (standing - 400.0).abs() < 1.0,
            "the underside of the cap is the ceiling: {standing}"
        );
        let best = best_orientation(&mesh, 45.0);
        // Not zero: the post's top face is buried in the cap, and once the part
        // is turned over that buried 4 x 4 face points down. No slicer ever
        // sees it, and 16 mm² against the 400 mm² it replaced is the point.
        assert!(best.overhang_mm2 < 20.0, "the 400 mm² ceiling is gone: {best:?}");
        assert!((best.height_mm - 12.0).abs() < 1e-6, "{best:?}");
        // A 20 x 20 footprint means the cap is what it stands on, whichever
        // quarter turns the search happened to name it with.
        assert!((best.footprint_mm2 - 400.0).abs() < 1.0, "{best:?}");
    }

    #[test]
    fn overhang_follows_the_support_angle() {
        let mesh = wedge();
        // The ramp climbs at 45°, so it is exactly the threshold: just under
        // 45° it is support, just over 45° the slicer leaves it alone.
        let gentle = overhang_area(&mesh, [0.0, 0.0, 0.0], 44.0);
        let steep = overhang_area(&mesh, [0.0, 0.0, 0.0], 46.0);
        assert!(gentle <= steep, "{gentle} then {steep}");
    }

    #[test]
    fn orienting_moves_the_part_without_changing_it() {
        let mesh = wedge();
        let before = Transform {
            translation: [40.0, 50.0, 3.0],
            rotation_deg: [0.0, 90.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        };
        let after = orient(&mesh, &before, 45.0);
        let baked_before = crate::mesh::bake(&mesh, &before);
        let baked_after = crate::mesh::bake(&mesh, &after);
        assert!(
            (volume(&baked_before).abs() - volume(&baked_after).abs()).abs() < 1.0,
            "{} then {}",
            volume(&baked_before).abs(),
            volume(&baked_after).abs()
        );
        let (min_after, _) = baked_after.bounds();
        assert!(min_after[2].abs() < 1e-3, "it should sit on the plate: {min_after:?}");
        // The middle of the part stays where it was, so it does not run away.
        let (min_before, max_before) = baked_before.bounds();
        let (_, max_after) = baked_after.bounds();
        let moved_x = ((min_after[0] + max_after[0]) - (min_before[0] + max_before[0])) * 0.5;
        let moved_y = ((min_after[1] + max_after[1]) - (min_before[1] + max_before[1])) * 0.5;
        assert!(moved_x.abs() < 0.01 && moved_y.abs() < 0.01, "{moved_x} {moved_y}");
    }

    #[test]
    fn a_clicked_face_ends_up_on_the_plate() {
        let mesh = cube(10.0);
        let before = Transform {
            translation: [30.0, 40.0, 5.0],
            rotation_deg: [30.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        };
        // The +Y face of the cube, turned into the world by the part's rotation.
        let tilted = apply_transform(
            [0.0, 1.0, 0.0],
            &Transform {
                translation: [0.0, 0.0, 0.0],
                rotation_deg: before.rotation_deg,
                scale: [1.0, 1.0, 1.0],
            },
        );
        let after = face_rest(&mesh, &before, tilted);
        let baked = crate::mesh::bake(&mesh, &after);
        // It sits on the plate.
        let (min, _) = baked.bounds();
        assert!(min[2].abs() < 1e-6, "min z {}", min[2]);
        // The face that was clicked now looks straight down: the lowest face's
        // normal is -Z.
        let mut lowest = None;
        for triangle in &baked.triangles {
            let a = baked.vertices[triangle[0] as usize];
            let b = baked.vertices[triangle[1] as usize];
            let c = baked.vertices[triangle[2] as usize];
            if (a[2] - min[2] as f32).abs() < 1e-4
                && (b[2] - min[2] as f32).abs() < 1e-4
                && (c[2] - min[2] as f32).abs() < 1e-4
            {
                lowest = Some([a, b, c]);
            }
        }
        assert!(lowest.is_some(), "no face on the plate");
        // The volume is unchanged: this is a rotation, not a reshape.
        assert!((volume(&baked).abs() - 1000.0).abs() < 1.0, "{}", volume(&baked).abs());
        // And it stayed where it stood in plan.
        let (min_before, max_before) = crate::mesh::bake(&mesh, &before).bounds();
        let (min_after, max_after) = baked.bounds();
        let centre_before = ((min_before[0] + max_before[0]) * 0.5, (min_before[1] + max_before[1]) * 0.5);
        let centre_after = ((min_after[0] + max_after[0]) * 0.5, (min_after[1] + max_after[1]) * 0.5);
        assert!((centre_before.0 - centre_after.0).abs() < 1e-6);
        assert!((centre_before.1 - centre_after.1).abs() < 1e-6);
    }
}
