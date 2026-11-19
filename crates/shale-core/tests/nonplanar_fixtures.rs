//! Shared non-planar integration / matrix meshes.

use std::path::PathBuf;

use shale_core::catalog::Catalog;
use shale_core::engine::SlicingEngine;
use shale_core::job::{self, PlacedModel};
use shale_core::mesh::{self, Transform, TriangleMesh};
use shale_core::primitives;
use shale_core::resolver::UserSettings;
use shale_core::{NonplanarMode, NonplanarOptions};

const PILLAR_PRINTER: &str = "bambu_lab_a1";
const PILLAR_VARIANT: &str = "0.4 mm";
const PILLAR_QUALITY: &str = "0_20mm_standard_bbl_a1";
const PILLAR_MATERIAL: &str = "bambu_lab_pla_basic";

fn mesh_for_nonplanar(mesh: &TriangleMesh) -> shale_nonplanar::mesh::Mesh {
    shale_nonplanar::mesh::Mesh {
        vertices: mesh
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: mesh.triangles.clone(),
    }
}

pub fn merge_meshes(a: &TriangleMesh, b: &TriangleMesh) -> TriangleMesh {
    let mut mesh = a.clone();
    let offset = mesh.vertices.len() as u32;
    mesh.vertices.extend(b.vertices.iter().copied());
    for tri in &b.triangles {
        mesh.triangles
            .push([tri[0] + offset, tri[1] + offset, tri[2] + offset]);
    }
    mesh
}

/// Per-slice overrides so Cura finishes the tall pillar before the low dome (`by object`).
pub fn pillar_beside_dome_slice_settings() -> Vec<(String, String)> {
    vec![
        ("ironing_enabled".into(), "false".into()),
        ("print_sequence".into(), "one_at_a_time".into()),
    ]
}

/// Dome + pedestal and a tall pillar as **two print objects** (`print_sequence` by object)
/// so low-Z dome travels pass within the conservative A1 head radius (~12 mm body)
/// of pillar material already at full height.
pub fn pillar_beside_dome_parts() -> (TriangleMesh, Transform, TriangleMesh, Transform) {
    let pedestal = primitives::box_solid(55.0, 45.0, 2.0);
    let dome = mesh::bake(
        &primitives::sphere(12.0),
        &Transform {
            translation: [0.0, 0.0, 2.0],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        },
    );
    let dome_mesh = merge_meshes(&pedestal, &dome);
    let dome_transform = mesh::place_on_bed(&dome_mesh, 256.0, 256.0, false);

    let pillar_mesh = primitives::box_solid(10.0, 10.0, 48.0);
    let mut pillar_transform = mesh::place_on_bed(&pillar_mesh, 256.0, 256.0, false);
    // Offset toward +X so dome shell travels skim beside the pillar footprint.
    pillar_transform.translation[0] += 6.0;
    pillar_transform.translation[1] += 2.0;

    (dome_mesh, dome_transform, pillar_mesh, pillar_transform)
}

/// Single merged mesh (legacy); prefer [`pillar_beside_dome_parts`] for collision e2e.
pub fn pillar_beside_dome() -> TriangleMesh {
    let (dome_mesh, _, pillar_mesh, _) = pillar_beside_dome_parts();
    merge_meshes(&dome_mesh, &pillar_mesh)
}

fn slice_deferred_engine_gcode(
    catalog: &Catalog,
    engine: &dyn SlicingEngine,
    printer_id: &str,
    placed: &[PlacedModel],
    path: PathBuf,
) -> (String, String, shale_core::resolver::ResolvedSettings) {
    let settings = UserSettings {
        layer_height: Some(0.2),
        nonplanar: NonplanarOptions {
            mode: NonplanarMode::CurvedTops,
            max_angle_deg: 10.0,
            defer_post_pass: true,
            ..NonplanarOptions::default()
        },
        ..UserSettings::default()
    };
    let out = job::slice(
        catalog,
        engine,
        printer_id,
        Some(PILLAR_VARIANT.into()),
        Some(PILLAR_QUALITY.into()),
        &[Some(PILLAR_MATERIAL.into())],
        settings,
        placed,
        &[],
        None,
        &[],
        path,
        &mut |_| {},
    )
    .expect("deferred slice");
    (
        out.engine_gcode.expect("engine gcode"),
        out.planar_baseline_gcode.expect("planar baseline"),
        out.resolved,
    )
}

/// Tall pillar G-code first, then dome — real Cura slices concatenated for envelope flattening.
pub fn slice_pillar_beside_dome_sequential(
    catalog: &Catalog,
    engine: &dyn SlicingEngine,
    printer_id: &str,
    tag: &str,
) -> (String, shale_nonplanar::NonplanarReport) {
    let (dome_mesh, dome_tf, pillar_mesh, pillar_tf) = pillar_beside_dome_parts();
    let pid = std::process::id();
    let (p_eng, _p_planar, resolved) = slice_deferred_engine_gcode(
        catalog,
        engine,
        printer_id,
        &[PlacedModel {
            name: "pillar".into(),
            mesh: pillar_mesh.clone(),
            transform: pillar_tf.clone(),
            settings: vec![("ironing_enabled".into(), "false".into())],
            slot: None,
        }],
        std::env::temp_dir().join(format!("shale-pillar-{tag}-p-{pid}.gcode")),
    );
    let (d_eng, _d_planar, _) = slice_deferred_engine_gcode(
        catalog,
        engine,
        printer_id,
        &[PlacedModel {
            name: "dome-pedestal".into(),
            mesh: dome_mesh.clone(),
            transform: dome_tf.clone(),
            settings: vec![("ironing_enabled".into(), "false".into())],
            slot: None,
        }],
        std::env::temp_dir().join(format!("shale-pillar-{tag}-d-{pid}.gcode")),
    );
    let deformed = format!("{p_eng}\n{d_eng}");
    let (dome_mesh2, _, pillar_mesh2, _) = pillar_beside_dome_parts();
    let merged = merge_meshes(&dome_mesh2, &pillar_mesh2);
    let merged_tf = mesh::place_on_bed(&merged, 256.0, 256.0, false);
    let planar_merged_path =
        std::env::temp_dir().join(format!("shale-pillar-{tag}-merged-planar-{pid}.gcode"));
    let planar_settings = UserSettings {
        layer_height: Some(0.2),
        nonplanar: NonplanarOptions::default(),
        ..UserSettings::default()
    };
    job::slice(
        catalog,
        engine,
        printer_id,
        Some(PILLAR_VARIANT.into()),
        Some(PILLAR_QUALITY.into()),
        &[Some(PILLAR_MATERIAL.into())],
        planar_settings,
        &[PlacedModel {
            name: "pillar-beside-dome".into(),
            mesh: merged,
            transform: merged_tf,
            settings: vec![("ironing_enabled".into(), "false".into())],
            slot: None,
        }],
        &[],
        None,
        &[],
        planar_merged_path.clone(),
        &mut |_| {},
    )
    .expect("merged planar");
    let planar = std::fs::read_to_string(planar_merged_path).expect("merged planar gcode");
    let reference = shale_nonplanar::merge_meshes(&[
        mesh_for_nonplanar(&mesh::bake(&pillar_mesh, &pillar_tf)),
        mesh_for_nonplanar(&mesh::bake(&dome_mesh, &dome_tf)),
    ]);
    let layer_height = resolved.summary.layer_height;
    let max_feed = resolved
        .global
        .get("machine_max_feedrate_x")
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| (v * 60.0).max(600.0))
        .unwrap_or(600.0);
    let opts = NonplanarOptions {
        mode: NonplanarMode::CurvedTops,
        max_angle_deg: 10.0,
        ..NonplanarOptions::default()
    };
    shale_nonplanar::apply_post_pass(
        &deformed,
        &planar,
        &reference,
        &opts,
        layer_height,
        &[],
        printer_id,
        shale_nonplanar::MachineLimits {
            z_max_mm: resolved.summary.machine_height.max(1.0),
            max_feed_mm_per_min: max_feed,
        },
    )
}