//! Write `examples/dome.stl` and `examples/chevron-slab.stl` for `scripts/bench-auto-nonplanar.sh`.

use std::path::PathBuf;

use shale_core::mesh::{bake, write_binary_stl, TriangleMesh};
use shale_core::primitives;
use shale_core::Transform;

fn merge_meshes(a: &TriangleMesh, b: &TriangleMesh) -> TriangleMesh {
    let mut mesh = a.clone();
    let offset = mesh.vertices.len() as u32;
    mesh.vertices.extend_from_slice(&b.vertices);
    for tri in &b.triangles {
        mesh.triangles.push([tri[0] + offset, tri[1] + offset, tri[2] + offset]);
    }
    mesh
}

/// Pedestal plus a forward shelf: horizontal underside at z=15 (cantilever).
fn overhang_shelf() -> TriangleMesh {
    let base = primitives::box_solid(30.0, 20.0, 15.0);
    let shelf = bake(
        &primitives::box_solid(30.0, 10.0, 10.0),
        &Transform {
            translation: [0.0, 15.0, 15.0],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        },
    );
    merge_meshes(&base, &shelf)
}

fn chevron_slab() -> TriangleMesh {
    let tilt = 12.0f32.to_radians();
    let base_h = 10.0;
    let base = primitives::box_solid(40.0, 24.0, base_h);
    let mut slab = primitives::box_solid(40.0, 24.0, 1.2);
    for v in slab.vertices.iter_mut() {
        v[2] += v[0] * tilt.tan();
    }
    let slab = bake(
        &slab,
        &Transform {
            translation: [0.0, 0.0, base_h],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        },
    );
    merge_meshes(&base, &slab)
}

#[test]
fn export_bench_stls() {
    if std::env::var_os("SHALE_EXPORT_BENCH_STLS").is_none() {
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    write_binary_stl(&root.join("dome.stl"), &primitives::sphere(12.0)).expect("dome");
    write_binary_stl(&root.join("chevron-slab.stl"), &chevron_slab()).expect("chevron");
    write_binary_stl(&root.join("overhang-shelf.stl"), &overhang_shelf()).expect("overhang shelf");
}