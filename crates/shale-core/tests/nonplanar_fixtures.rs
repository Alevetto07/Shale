//! Shared non-planar integration / matrix meshes.
#![allow(dead_code)] // Each test binary includes this module and uses a different subset.

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

/// The printer's own 0.20 mm standard process. The hard-coded A1 key is wrong for
/// the A1 mini and the P/X/H envelope printers, whose Orca keys end in their own
/// model suffix.
fn pillar_quality(catalog: &Catalog, printer_id: &str) -> String {
    let qualities = catalog.qualities_for(printer_id, Some(PILLAR_VARIANT));
    qualities
        .iter()
        .find(|q| q.quality_type.contains("0_20") || q.quality_type.contains("0.20"))
        .or_else(|| qualities.first())
        .map(|q| q.quality_type.clone())
        .unwrap_or_else(|| PILLAR_QUALITY.into())
}

fn limits_from(
    resolved: &shale_core::resolver::ResolvedSettings,
) -> shale_nonplanar::MachineLimits {
    let max_feed = resolved
        .global
        .get("machine_max_feedrate_x")
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| (v * 60.0).max(600.0))
        .unwrap_or(600.0);
    shale_nonplanar::MachineLimits {
        z_max_mm: resolved.summary.machine_height.max(1.0),
        max_feed_mm_per_min: max_feed,
    }
}

/// Pillar and dome as **two print objects**, sliced by one deferred job.
///
/// Both parts are deformed with the single plate field `job::slice` planned from
/// the merged machine-space geometry, then mapped back with that same field: the
/// inverse here is the inverse of the deformation CuraEngine sliced.
pub fn slice_pillar_beside_dome_sequential(
    catalog: &Catalog,
    engine: &dyn SlicingEngine,
    printer_id: &str,
    tag: &str,
) -> (String, shale_nonplanar::NonplanarReport) {
    let (dome_mesh, dome_tf, pillar_mesh, pillar_tf) = pillar_beside_dome_parts();
    let pid = std::process::id();
    let options = NonplanarOptions {
        mode: NonplanarMode::CurvedTops,
        max_angle_deg: 10.0,
        defer_post_pass: true,
        ..NonplanarOptions::default()
    };
    let settings = UserSettings {
        layer_height: Some(0.2),
        nonplanar: options.clone(),
        ..UserSettings::default()
    };
    let per_model = pillar_beside_dome_slice_settings();
    let out = job::slice(
        catalog,
        engine,
        printer_id,
        Some(PILLAR_VARIANT.into()),
        Some(pillar_quality(catalog, printer_id)),
        &[Some(PILLAR_MATERIAL.into())],
        settings,
        &[
            PlacedModel {
                name: "pillar".into(),
                mesh: pillar_mesh,
                transform: pillar_tf,
                settings: per_model.clone(),
                slot: None,
            },
            PlacedModel {
                name: "dome-pedestal".into(),
                mesh: dome_mesh,
                transform: dome_tf,
                settings: per_model,
                slot: None,
            },
        ],
        &[],
        None,
        &[],
        std::env::temp_dir().join(format!("shale-pillar-{tag}-deferred-{pid}.gcode")),
        &mut |_| {},
    )
    .expect("deferred slice");
    let deformed = out.engine_gcode.clone().expect("engine gcode");
    let planar = out.planar_baseline_gcode.clone().expect("planar baseline");
    let context = out
        .nonplanar_context
        .clone()
        .expect("deferred job carries its field");
    let (mapped, report) = shale_nonplanar::apply_post_pass(shale_nonplanar::PostPassArgs {
        deformed_slice_gcode: &deformed,
        planar_baseline_gcode: &planar,
        context: &context,
        options: &options,
        printer_id,
        limits: limits_from(&out.resolved),
    })
    .expect("non-strict post-pass never errors");
    (mapped, report)
}
