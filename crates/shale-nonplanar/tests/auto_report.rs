//! Deterministic placement report smoke test (no Cura).

use shale_nonplanar::config::{NonplanarMode, NonplanarOptions};
use shale_nonplanar::mesh::Mesh;
use shale_nonplanar::{plan_auto_placement, DetectedFeatureKind};

fn box_mesh(w: f64, d: f64, h: f64) -> Mesh {
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

#[test]
fn emit_auto_placement_report_for_models() {
    if std::env::var_os("SHALE_WRITE_AUTO_REPORT").is_none() {
        return;
    }
    fn sphere_mesh(radius: f64) -> Mesh {
        let rings = 12usize;
        let sides = 24usize;
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

    fn tilted_top_slab(w: f64, d: f64, t: f64, tilt_deg: f64) -> Mesh {
        let tilt = tilt_deg.to_radians();
        let mut mesh = box_mesh(w, d, t);
        for v in &mut mesh.vertices {
            v[2] += v[0] * tilt.tan();
        }
        mesh
    }
    let opts = NonplanarOptions {
        mode: NonplanarMode::Auto,
        max_angle_deg: 10.0,
        grid_step_mm: 0.35,
        ..NonplanarOptions::default()
    };
    let models = [
        ("flat_plate", box_mesh(40.0, 40.0, 4.0)),
        ("dome", sphere_mesh(12.0)),
        ("ramp_9deg", tilted_top_slab(36.0, 20.0, 1.2, 9.0)),
    ];
    let mut out = serde_json::Map::new();
    for (name, mesh) in models {
        let plan = plan_auto_placement(&mesh, &[], &opts, 0.2);
        out.insert(
            name.into(),
            serde_json::to_value(&plan).expect("serialize plan"),
        );
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/nonplanar/auto-placement-report.json");
    std::fs::create_dir_all(path.parent().unwrap()).ok();
    std::fs::write(&path, serde_json::to_vec_pretty(&out).unwrap()).unwrap();
}

#[test]
fn auto_report_flat_cube_has_no_bands() {
    let opts = NonplanarOptions {
        mode: NonplanarMode::Auto,
        max_angle_deg: 10.0,
        ..NonplanarOptions::default()
    };
    let plan = plan_auto_placement(&box_mesh(20.0, 20.0, 10.0), &[], &opts, 0.2);
    assert_eq!(plan.enabled_surface_count, 0);
    assert!(plan.detection.features.iter().any(|f| f.kind == DetectedFeatureKind::FlatTop));
}