//! Minimal triangle mesh type (no dependency on `shale-core`).

#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for vertex in &self.vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex[axis]);
                max[axis] = max[axis].max(vertex[axis]);
            }
        }
        if self.vertices.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        (min, max)
    }
}

pub fn merge_meshes(meshes: &[Mesh]) -> Mesh {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for mesh in meshes {
        let offset = vertices.len() as u32;
        vertices.extend(mesh.vertices.iter().copied());
        for tri in &mesh.triangles {
            triangles.push([tri[0] + offset, tri[1] + offset, tri[2] + offset]);
        }
    }
    Mesh { vertices, triangles }
}