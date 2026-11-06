//! The solids a slicer can put on the bed without a file.
//!
//! Every slicer ships these: a cube to test first-layer squish, a cylinder for
//! a hole that should be round, a cone for overhangs, a sphere for surface
//! finish. Bambu Studio's right-click menu calls it "Add primitive". The meshes
//! here are closed and wound counter-clockwise seen from outside, so the signed
//! volume comes out positive and the rest of the pipeline (support, export,
//! boolean-free things that assume a solid) can take them without repair.
//!
//! They all stand on `z = 0` and are centred on the plate origin in x and y,
//! which is where a part dropped on a bed belongs.

use crate::mesh::TriangleMesh;

const SIDES: usize = 64;

/// A box, `width` across x, `depth` across y, `height` up z.
pub fn box_solid(width: f64, depth: f64, height: f64) -> TriangleMesh {
    let width = width.max(0.1);
    let depth = depth.max(0.1);
    let height = height.max(0.1);
    let (x, y) = (width as f32 / 2.0, depth as f32 / 2.0);
    let z = height as f32;
    let mut mesh = TriangleMesh {
        vertices: vec![
            [-x, -y, 0.0],
            [x, -y, 0.0],
            [x, y, 0.0],
            [-x, y, 0.0],
            [-x, -y, z],
            [x, -y, z],
            [x, y, z],
            [-x, y, z],
        ],
        triangles: Vec::new(),
    };
    let faces: [[u32; 3]; 12] = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    mesh.triangles.extend_from_slice(&faces);
    mesh
}

/// A cylinder standing on its base.
pub fn cylinder(radius: f64, height: f64) -> TriangleMesh {
    let radius = radius.max(0.05) as f32;
    let height = height.max(0.1) as f32;
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    // Two rings of vertices, bottom then top, sharing the seam vertex so the
    // wall closes without a duplicated corner.
    for z in [0.0f32, height] {
        for side in 0..SIDES {
            let angle = side as f32 / SIDES as f32 * std::f32::consts::TAU;
            mesh.vertices.push([radius * angle.cos(), radius * angle.sin(), z]);
        }
    }
    let ring = SIDES as u32;
    for side in 0..ring {
        let next = (side + 1) % ring;
        // out through the wall, counter-clockwise from outside
        mesh.triangles.push([side, next, ring + next]);
        mesh.triangles.push([side, ring + next, ring + side]);
    }
    // Caps as a fan: the bottom looks down, so its winding is reversed.
    let bottom = mesh.vertices.len() as u32;
    mesh.vertices.push([0.0, 0.0, 0.0]);
    let top = bottom + 1;
    mesh.vertices.push([0.0, 0.0, height]);
    for side in 0..ring {
        let next = (side + 1) % ring;
        mesh.triangles.push([bottom, next, side]);
        mesh.triangles.push([top, ring + side, ring + next]);
    }
    mesh
}

/// A cone: a circle at the bed, a point above it.
pub fn cone(radius: f64, height: f64) -> TriangleMesh {
    let radius = radius.max(0.05) as f32;
    let height = height.max(0.1) as f32;
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    for side in 0..SIDES {
        let angle = side as f32 / SIDES as f32 * std::f32::consts::TAU;
        mesh.vertices.push([radius * angle.cos(), radius * angle.sin(), 0.0]);
    }
    let ring = SIDES as u32;
    let apex = ring;
    mesh.vertices.push([0.0, 0.0, height]);
    let centre = apex + 1;
    mesh.vertices.push([0.0, 0.0, 0.0]);
    for side in 0..ring {
        let next = (side + 1) % ring;
        mesh.triangles.push([side, next, apex]);
        mesh.triangles.push([centre, next, side]);
    }
    mesh
}

/// A sphere on the bed: mid-plane at `radius`, the bottom touching `z = 0`.
pub fn sphere(radius: f64) -> TriangleMesh {
    let radius = radius.max(0.1) as f32;
    let rings = 24usize;
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    let bottom = mesh.vertices.len() as u32;
    mesh.vertices.push([0.0, 0.0, 0.0]);
    for ring in 1..rings {
        let phi = std::f32::consts::PI * ring as f32 / rings as f32;
        let z = radius - radius * phi.cos();
        let r = radius * phi.sin();
        for side in 0..SIDES {
            let angle = side as f32 / SIDES as f32 * std::f32::consts::TAU;
            mesh.vertices.push([r * angle.cos(), r * angle.sin(), z]);
        }
    }
    let top = mesh.vertices.len() as u32;
    mesh.vertices.push([0.0, 0.0, 2.0 * radius]);
    let ring_vertices = SIDES as u32;

    // Bottom fan: from the pole up to the first ring.
    for side in 0..ring_vertices {
        let next = (side + 1) % ring_vertices;
        mesh.triangles.push([bottom, 1 + next, 1 + side]);
    }
    // One band of quads per pair of rings.
    for ring in 1..(rings as u32 - 1) {
        let lower = 1 + (ring - 1) * ring_vertices;
        let upper = lower + ring_vertices;
        for side in 0..ring_vertices {
            let next = (side + 1) % ring_vertices;
            mesh.triangles.push([lower + side, lower + next, upper + next]);
            mesh.triangles.push([lower + side, upper + next, upper + side]);
        }
    }
    // Top fan: last ring up to the pole.
    let last = 1 + (rings as u32 - 2) * ring_vertices;
    for side in 0..ring_vertices {
        let next = (side + 1) % ring_vertices;
        mesh.triangles.push([last + side, last + next, top]);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::signed_volume;

    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() <= right.abs() * 0.02
    }

    #[test]
    fn a_box_is_the_volume_it_says() {
        let mesh = box_solid(20.0, 30.0, 10.0);
        assert_eq!(mesh.triangles.len(), 12);
        assert!(close(signed_volume(&mesh), 20.0 * 30.0 * 10.0));
    }

    #[test]
    fn a_cylinder_holds_pi_r_squared_h() {
        let mesh = cylinder(10.0, 20.0);
        let want = std::f64::consts::PI * 10.0 * 10.0 * 20.0;
        assert!(close(signed_volume(&mesh), want), "{} vs {}", signed_volume(&mesh), want);
        let (min, max) = mesh.bounds();
        assert!((min[2] - 0.0).abs() < 1e-4, "stands on the bed");
        assert!((max[2] - 20.0).abs() < 1e-3);
    }

    #[test]
    fn a_cone_is_a_third_of_the_cylinder() {
        let mesh = cone(10.0, 20.0);
        let want = std::f64::consts::PI * 10.0 * 10.0 * 20.0 / 3.0;
        assert!(close(signed_volume(&mesh), want), "{} vs {}", signed_volume(&mesh), want);
    }

    #[test]
    fn a_sphere_is_four_thirds_pi_r_cubed() {
        let mesh = sphere(10.0);
        let want = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        assert!(close(signed_volume(&mesh), want), "{} vs {}", signed_volume(&mesh), want);
    }
}
