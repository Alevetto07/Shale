//! Booleans through Manifold, and why they are here.
//!
//! The BSP in `csg.rs` is exact on paper and quadratic in practice. It also
//! leaves T-junctions wherever a polygon is split without its neighbour being
//! split with it — 4 737 of them on a 7 800-triangle cut, 16 444 at 32 500 —
//! which is a hole to every slicer that trusts the winding.
//!
//! [Manifold](https://github.com/elalish/manifold) (Apache-2.0, by Emmett Lalish
//! and the Manifold contributors) is a C++ library that guarantees its outputs
//! are 2-manifold: every edge is shared by exactly two triangles, which is what
//! "watertight" means to a slicer. It reaches that by keeping exact plane
//! information through the operation rather than rounding to f32 on every split.
//! This module is the thin Rust edge of it — the `manifold3d` 0.4.1 crate, which
//! wraps the pinned upstream C API of v3.5.3 — and nothing more: no transforms,
//! no units, no policy.
//!
//! The policy lives in `csg.rs`: the library is the default answer and the BSP
//! is what happens when the library refuses an input (a mesh with holes, a
//! non-manifold bundle of shells, a self-intersection it cannot resolve). The
//! caller is told which one answered, because a silent downgrade to a
//! two-hundred-times-slower path is the kind of thing that should be visible.
//!
//! Two conversions matter and both are lossy in the same direction:
//!
//! * Shale stores f32 vertices; Manifold wants f32 for `MeshGL`, so the vertices
//!   travel unchanged and the result is rounded back to f32 once, at the end.
//! * Shale's `TriangleMesh` may carry a winding that points inwards, because an
//!   STL does not promise otherwise. Manifold reads winding as inside/outside, so
//!   a negative signed volume is flipped before it is handed over — otherwise
//!   every subtraction would come back as its complement.

use crate::csg::Op;
use crate::mesh::TriangleMesh;
use manifold3d::{Manifold, MeshGL};

/// The boolean, or `None` if Manifold would not take the input.
///
/// `None` is not an error to report to the user: it means "try the other
/// engine", which `csg.rs` does.
pub fn boolean(a: &TriangleMesh, b: &TriangleMesh, op: Op) -> Option<TriangleMesh> {
    let left = solid(a)?;
    let right = solid(b)?;
    let answer = match op {
        Op::Union => left.union(&right),
        Op::Difference => left.difference(&right),
        Op::Intersection => left.intersection(&right),
    };
    // Manifold reports laziness errors here rather than panicking: an invalid
    // result is an `Err` from `status()`, and the BSP gets its turn.
    answer.status().ok()?;
    Some(to_shale(&answer))
}

/// A `TriangleMesh` as a Manifold, or `None` if Manifold calls it not-manifold.
fn solid(mesh: &TriangleMesh) -> Option<Manifold> {
    // Fewer than four triangles cannot bound a volume.
    if mesh.triangles.len() < 4 || mesh.vertices.is_empty() {
        return None;
    }
    let mut vertices = Vec::with_capacity(mesh.vertices.len() * 3);
    let vertices = {
        for vertex in &mesh.vertices {
            vertices.extend_from_slice(vertex);
        }
        vertices
    };
    let mut indices = Vec::with_capacity(mesh.triangles.len() * 3);
    for triangle in &mesh.triangles {
        indices.extend_from_slice(triangle);
    }

    let mut oriented = None;
    if signed_volume(mesh) < 0.0 {
        // Inside-out: Manifold would read the solid as its complement.
        let mut flipped = TriangleMesh {
            vertices: mesh.vertices.clone(),
            triangles: mesh
                .triangles
                .iter()
                .map(|triangle| [triangle[0], triangle[2], triangle[1]])
                .collect(),
        };
        flipped.vertices.shrink_to_fit();
        oriented = Some(flipped);
    }
    let (vertices, indices) = match &oriented {
        Some(flipped) => {
            let mut flat = Vec::with_capacity(flipped.vertices.len() * 3);
            for vertex in &flipped.vertices {
                flat.extend_from_slice(vertex);
            }
            let mut idx = Vec::with_capacity(flipped.triangles.len() * 3);
            for triangle in &flipped.triangles {
                idx.extend_from_slice(triangle);
            }
            (flat, idx)
        }
        None => (vertices, indices),
    };

    let welded = MeshGL::new(&vertices, 3, &indices).ok()?.merge();
    Manifold::from_meshgl(&welded).ok()
}

/// The answer, back in Shale's own shape.
fn to_shale(manifold: &Manifold) -> TriangleMesh {
    let (vertices, _count, triangles) = manifold.to_mesh_f32();
    let mut mesh = TriangleMesh {
        vertices: vertices.as_chunks::<3>().0.to_vec(),
        triangles: triangles.as_chunks::<3>().0.to_vec(),
    };
    mesh.vertices.shrink_to_fit();
    mesh.triangles.shrink_to_fit();
    mesh
}

/// Six times the signed volume of a closed mesh, by the divergence theorem.
fn signed_volume(mesh: &TriangleMesh) -> f64 {
    let mut total = 0.0;
    for triangle in &mesh.triangles {
        let a = mesh.vertices[triangle[0] as usize].map(f64::from);
        let b = mesh.vertices[triangle[1] as usize].map(f64::from);
        let c = mesh.vertices[triangle[2] as usize].map(f64::from);
        total += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    total / 6.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube(size: f64) -> TriangleMesh {
        // A boxed() helper lives in csg.rs's tests; this module only needs one
        // shape, so it builds its own.
        let mut vertices = Vec::new();
        for x in [0.0f32, size as f32] {
            for y in [0.0f32, size as f32] {
                for z in [0.0f32, size as f32] {
                    vertices.push([x, y, z]);
                }
            }
        }
        // Index pairs: 0..8 in (x, y, z) bit order.
        let index = |x: usize, y: usize, z: usize| (x * 4 + y * 2 + z) as u32;
        let mut triangles = Vec::new();
        let mut quad = |a: u32, b: u32, c: u32, d: u32| {
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        };
        quad(index(0, 0, 0), index(0, 1, 0), index(0, 1, 1), index(0, 0, 1));
        quad(index(1, 0, 0), index(1, 0, 1), index(1, 1, 1), index(1, 1, 0));
        quad(index(0, 0, 0), index(0, 0, 1), index(1, 0, 1), index(1, 0, 0));
        quad(index(0, 1, 0), index(1, 1, 0), index(1, 1, 1), index(0, 1, 1));
        quad(index(0, 0, 0), index(1, 0, 0), index(1, 1, 0), index(0, 1, 0));
        quad(index(0, 0, 1), index(0, 1, 1), index(1, 1, 1), index(1, 0, 1));
        let mut mesh = TriangleMesh {
            vertices,
            triangles,
        };
        // The quads above were wound by index order, not by intent. A fixture
        // that is inside out would pass a round trip for the wrong reason, so
        // it is turned the right way out once, here.
        if signed_volume(&mesh) < 0.0 {
            for triangle in &mut mesh.triangles {
                triangle.swap(1, 2);
            }
        }
        mesh
    }

    #[test]
    fn a_box_survives_the_round_trip_exactly() {
        let mesh = cube(10.0);
        let volume = signed_volume(&mesh);
        assert!((volume - 1000.0).abs() < 1e-3, "the fixture is a 1000 mm3 box: {volume}");
        let welded = solid(&mesh).expect("a box is manifold");
        assert!((welded.volume() - 1000.0).abs() < 1e-3);
        let back = to_shale(&welded);
        assert_eq!(back.vertices.len(), 8);
        assert_eq!(back.triangles.len(), 12);
    }

    #[test]
    fn an_inside_out_box_is_flipped_rather_than_read_as_a_hole() {
        let mut mesh = cube(10.0);
        for triangle in &mut mesh.triangles {
            triangle.swap(1, 2);
        }
        assert!(signed_volume(&mesh) < 0.0);
        let welded = solid(&mesh).expect("still manifold, just wound the wrong way");
        assert!(
            (welded.volume() - 1000.0).abs() < 1e-3,
            "volume must be positive after the flip: {}",
            welded.volume()
        );
    }

    #[test]
    fn an_open_mesh_is_refused_so_the_caller_can_fall_back() {
        let mut mesh = cube(10.0);
        mesh.triangles.pop(); // one triangle short of closed
        assert!(solid(&mesh).is_none(), "a hole is not a solid");
    }
}
