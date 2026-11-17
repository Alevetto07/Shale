#[cfg(test)]
use crate::mesh::Mesh;

#[cfg(test)]
pub fn box_mesh(w: f64, d: f64, h: f64) -> Mesh {
    let (x, y) = (w / 2.0, d / 2.0);
    let z = h;
    let vertices = vec![
        [-x, -y, 0.0],
        [x, -y, 0.0],
        [x, y, 0.0],
        [-x, y, 0.0],
        [-x, -y, z],
        [x, -y, z],
        [x, y, z],
        [-x, y, z],
    ];
    let triangles = vec![
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
    Mesh { vertices, triangles }
}

#[cfg(test)]
pub fn sphere_mesh(radius: f64) -> Mesh {
    let rings = 16usize;
    let sides = 32usize;
    let mut vertices = Vec::new();
    let bottom = 0u32;
    vertices.push([0.0, 0.0, 0.0]);
    for ring in 1..rings {
        let phi = std::f64::consts::PI * ring as f64 / rings as f64;
        let z = radius - radius * phi.cos();
        let r = radius * phi.sin();
        for side in 0..sides {
            let angle = side as f64 / sides as f64 * std::f64::consts::TAU;
            vertices.push([r * angle.cos(), r * angle.sin(), z]);
        }
    }
    let top = vertices.len() as u32;
    vertices.push([0.0, 0.0, 2.0 * radius]);
    let ring_vertices = sides as u32;
    let mut triangles = Vec::new();
    for side in 0..ring_vertices {
        let next = (side + 1) % ring_vertices;
        triangles.push([bottom, 1 + next, 1 + side]);
    }
    for ring in 1..(rings as u32 - 1) {
        let lower = 1 + (ring - 1) * ring_vertices;
        let upper = lower + ring_vertices;
        for side in 0..ring_vertices {
            let next = (side + 1) % ring_vertices;
            triangles.push([lower + side, lower + next, upper + next]);
            triangles.push([lower + side, upper + next, upper + side]);
        }
    }
    let last = 1 + (rings as u32 - 2) * ring_vertices;
    for side in 0..ring_vertices {
        let next = (side + 1) % ring_vertices;
        triangles.push([last + side, last + next, top]);
    }
    Mesh { vertices, triangles }
}

/// Low deck with a tall pillar beside it (for collision regression tests).
#[cfg(test)]
pub fn tilted_top_slab(w: f64, d: f64, t: f64, tilt_deg: f64) -> Mesh {
    let tilt = tilt_deg.to_radians();
    let mut mesh = box_mesh(w, d, t);
    for v in &mut mesh.vertices {
        v[2] += v[0] * tilt.tan();
    }
    mesh
}

/// Two pillars with a short horizontal slab between (bridge underside).
#[cfg(test)]
pub fn bridge_mesh() -> Mesh {
    let span = box_mesh(24.0, 8.0, 1.0);
    let mut mesh = box_mesh(6.0, 6.0, 20.0);
    let span_verts = span.vertices.len();
    for v in span.vertices {
        mesh.vertices.push([v[0] + 8.0, v[1], v[2] + 20.0]);
    }
    let o = (mesh.vertices.len() - span_verts) as u32;
    for t in span.triangles {
        mesh.triangles.push([t[0] + o, t[1] + o, t[2] + o]);
    }
    let mut pillar2 = box_mesh(6.0, 6.0, 20.0);
    for v in &mut pillar2.vertices {
        v[0] += 26.0;
    }
    let off = mesh.vertices.len() as u32;
    mesh.vertices.extend(pillar2.vertices);
    for t in pillar2.triangles {
        mesh.triangles.push([t[0] + off, t[1] + off, t[2] + off]);
    }
    mesh
}

#[cfg(test)]
pub fn cantilever_mesh() -> Mesh {
    let base = box_mesh(20.0, 20.0, 10.0);
    let arm = box_mesh(30.0, 8.0, 6.0);
    let mut mesh = base;
    let o = mesh.vertices.len() as u32;
    for v in arm.vertices {
        mesh.vertices.push([v[0] + 10.0, v[1], v[2] + 10.0]);
    }
    for t in arm.triangles {
        mesh.triangles.push([t[0] + o, t[1] + o, t[2] + o]);
    }
    mesh
}

#[cfg(test)]
pub fn pillar_beside_mesh() -> Mesh {
    let mut mesh = box_mesh(40.0, 20.0, 2.0);
    let pillar = box_mesh(8.0, 8.0, 30.0);
    let offset = mesh.vertices.len() as u32;
    for v in pillar.vertices {
        mesh.vertices.push([v[0] + 20.0, v[1], v[2]]);
    }
    for tri in pillar.triangles {
        mesh.triangles.push([
            tri[0] + offset,
            tri[1] + offset,
            tri[2] + offset,
        ]);
    }
    mesh
}