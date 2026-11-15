//! The ruler: two points on a part, and the distance between them.
//!
//! A click lands wherever the ray meets a triangle, but the point a person is
//! aiming at is almost always a vertex — a corner, the end of an edge — so the
//! click is pulled onto the nearest vertex within [`SNAP_MM`]. The arithmetic is
//! here rather than in the frontend because a ruler that is quietly wrong is
//! worse than no ruler: the number is the whole feature.

use crate::mesh::{bake, TriangleMesh};
use crate::Transform;

/// How far a click may be pulled onto a vertex, in millimetres. Two is about the
/// width of a nozzle and a little more, which is the scale of "I meant that
/// corner" rather than "I meant that face".
pub const SNAP_MM: f64 = 2.0;

/// A measured distance, in the axes and along the line.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Span {
    pub distance_mm: f64,
    /// `b - a` on each axis, so the sign says which way the second point lies.
    pub delta_mm: [f64; 3],
}

/// The distance and the per-axis deltas between two points.
pub fn span(a: [f64; 3], b: [f64; 3]) -> Span {
    let delta = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    Span {
        distance_mm: (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt(),
        delta_mm: delta,
    }
}

/// The nearest vertex to `point`, but only when it is within `limit`. A click in
/// the middle of a large flat face is not near any corner, and pulling it to one
/// would be a worse answer than leaving it where the ray landed.
pub fn nearest_vertex(vertices: &[[f64; 3]], point: [f64; 3], limit: f64) -> Option<[f64; 3]> {
    let mut best: Option<([f64; 3], f64)> = None;
    for vertex in vertices {
        let dx = vertex[0] - point[0];
        let dy = vertex[1] - point[1];
        let dz = vertex[2] - point[2];
        let distance = (dx * dx + dy * dy + dz * dz).sqrt();
        if distance > limit {
            continue;
        }
        match best {
            Some((_, best_distance)) if best_distance <= distance => {}
            _ => best = Some((*vertex, distance)),
        }
    }
    best.map(|(vertex, _)| vertex)
}

/// Snap a click on a placed part onto its nearest vertex.
///
/// The mesh is baked first, so the answer is in the same frame as the point the
/// ray reported: a part that has been turned, mirrored or slid across the plate
/// snaps onto where its corners actually are, not where the file left them.
pub fn snap(
    mesh: &TriangleMesh,
    transform: &Transform,
    point: [f64; 3],
    limit: f64,
) -> Option<[f64; 3]> {
    let world = bake(mesh, transform);
    let vertices: Vec<[f64; 3]> = world
        .vertices
        .iter()
        .map(|vertex| [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64])
        .collect();
    nearest_vertex(&vertices, point, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_three_four_five_is_five_millimetres() {
        let measured = span([0.0, 0.0, 0.0], [3.0, 4.0, 0.0]);
        assert!(close(measured.distance_mm, 5.0), "{:?}", measured);
        assert_eq!(measured.delta_mm, [3.0, 4.0, 0.0]);
    }

    #[test]
    fn the_deltas_carry_their_sign() {
        let measured = span([10.0, 10.0, 10.0], [7.0, 14.0, 2.0]);
        assert_eq!(measured.delta_mm, [-3.0, 4.0, -8.0]);
        assert!(close(measured.distance_mm, (9.0 + 16.0 + 64.0f64).sqrt()));
    }

    #[test]
    fn a_vertex_does_not_measure_itself() {
        let measured = span([2.0, 2.0, 2.0], [2.0, 2.0, 2.0]);
        assert!(close(measured.distance_mm, 0.0));
    }

    #[test]
    fn the_nearest_corner_wins() {
        let vertices = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]];
        assert_eq!(
            nearest_vertex(&vertices, [9.0, 0.4, 0.0], 2.0),
            Some([10.0, 0.0, 0.0])
        );
    }

    #[test]
    fn a_point_in_the_middle_of_a_face_is_not_pulled() {
        let vertices = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]];
        assert_eq!(nearest_vertex(&vertices, [5.0, 5.0, 0.0], 2.0), None);
    }

    #[test]
    fn the_snap_follows_the_placement() {
        // Two triangles that stand in for a corner at the mesh origin.
        let mesh = TriangleMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 4.0, 0.0],
                [0.0, 0.0, 4.0],
            ],
            triangles: vec![[0, 1, 2], [0, 1, 3]],
        };
        let moved = Transform {
            translation: [10.0, 0.0, 0.0],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        };
        let pointed = snap(&mesh, &moved, [10.6, 0.3, 0.0], SNAP_MM);
        assert_eq!(pointed, Some([10.0, 0.0, 0.0]));
        // The same click, without the placement, is nowhere near the origin.
        let unmoved = Transform {
            translation: [0.0, 0.0, 0.0],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        };
        assert_eq!(snap(&mesh, &unmoved, [10.6, 0.3, 0.0], SNAP_MM), None);
    }
}
