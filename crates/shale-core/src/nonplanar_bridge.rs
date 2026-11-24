//! Convert between `shale_core::mesh::TriangleMesh` and `shale_nonplanar::mesh::Mesh`.

use crate::mesh::TriangleMesh;

pub fn to_nonplanar(mesh: &TriangleMesh) -> shale_nonplanar::mesh::Mesh {
    shale_nonplanar::mesh::Mesh {
        vertices: mesh
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: mesh.triangles.clone(),
    }
}

pub fn from_nonplanar(mesh: &shale_nonplanar::mesh::Mesh) -> TriangleMesh {
    TriangleMesh {
        vertices: mesh
            .vertices
            .iter()
            .map(|v| [v[0] as f32, v[1] as f32, v[2] as f32])
            .collect(),
        triangles: mesh.triangles.clone(),
    }
}