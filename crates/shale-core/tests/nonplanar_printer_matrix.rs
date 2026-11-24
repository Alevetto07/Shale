//! Curved-top safety smoke: dome, chevron slab, t-wing, and collision fixture on every enveloped Bambu printer.

mod nonplanar_fixtures;

use std::path::PathBuf;

use nonplanar_fixtures::{
    pillar_beside_dome, slice_pillar_beside_dome_sequential,
};

use shale_core::job::{self, PlacedModel, SliceOutcome};
use shale_core::mesh::{self, Transform, TriangleMesh};
use shale_core::primitives;
use shale_core::resolver::UserSettings;
use shale_core::{Catalog, NonplanarMode, NonplanarOptions, ResourcePaths};

const MATERIAL: &str = "bambu_lab_pla_basic";
const VARIANT: &str = "0.4 mm";

fn configured() -> bool {
    std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some()
        && std::env::var_os("SHALE_ORCA_DIR").is_some()
}

fn artifact_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/nonplanar");
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn merge_meshes(a: &TriangleMesh, b: &TriangleMesh) -> TriangleMesh {
    let mut mesh = a.clone();
    let offset = mesh.vertices.len() as u32;
    mesh.vertices.extend(b.vertices.iter().copied());
    for tri in &b.triangles {
        mesh.triangles
            .push([tri[0] + offset, tri[1] + offset, tri[2] + offset]);
    }
    mesh
}

fn chevron_ramp() -> TriangleMesh {
    let tilt = 12.0f32.to_radians();
    let base_h = 10.0;
    let base = primitives::box_solid(40.0, 24.0, base_h);
    let mut slab = primitives::box_solid(40.0, 24.0, 1.2);
    for v in slab.vertices.iter_mut() {
        v[2] += v[0] * tilt.tan();
    }
    let slab = mesh::bake(
        &slab,
        &Transform {
            translation: [0.0, 0.0, base_h],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        },
    );
    merge_meshes(&base, &slab)
}

fn test_models() -> Vec<(&'static str, TriangleMesh)> {
    vec![
        ("dome", primitives::sphere(12.0)),
        ("chevron", chevron_ramp()),
        (
            "t-wing",
            mesh::load_mesh(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/t-wing.stl"))
                .expect("t-wing stl"),
        ),
        (
            "pillar-beside-dome",
            primitives::sphere(0.1), // placeholder; parts built in loop
        ),
    ]
}

#[derive(serde::Serialize)]
struct MatrixRow {
    printer_id: String,
    model: String,
    quality: String,
    planar_time_s: Option<f64>,
    curved_time_s: Option<f64>,
    collision_fixes: usize,
    travel_collisions_remaining: usize,
    planar_equivalence_ok: bool,
    safety_ok: bool,
    max_slope_deg: Option<f64>,
    safety_summary: String,
}

fn quality_for(catalog: &Catalog, printer_id: &str) -> Option<String> {
    catalog
        .qualities_for(printer_id, Some(VARIANT))
        .into_iter()
        .find(|q| q.quality_type.contains("0_20") || q.quality_type.contains("0.20"))
        .map(|q| q.quality_type.clone())
        .or_else(|| {
            catalog
                .qualities_for(printer_id, Some(VARIANT))
                .first()
                .map(|q| q.quality_type.clone())
        })
}

fn safety_summary(safety: &shale_nonplanar::config::SafetyReport) -> String {
    if safety.ok {
        format!(
            "ok; travel_collisions={}; feed_violations={}; slope_cap_ok={}",
            safety.travel_collisions_remaining,
            safety.feed_cap_violations,
            safety.slope_within_cap
        )
    } else {
        let kinds: Vec<_> = safety
            .violations
            .iter()
            .map(|v| v.kind.as_str())
            .collect();
        format!("FAIL: {:?}", kinds)
    }
}

#[test]
fn curved_top_safety_matrix() {
    if !configured() {
        return;
    }
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = shale_core::engine::CliEngine::from_env();
    let models = test_models();
    let mut rows = Vec::new();
    for printer_id in shale_nonplanar::envelope::envelope_catalogue() {
        let Some(quality) = quality_for(&catalog, printer_id) else {
            continue;
        };
        for (model_name, mesh) in &models {
            let placed = if *model_name == "pillar-beside-dome" {
                let merged = pillar_beside_dome();
                let transform = mesh::place_on_bed(&merged, 256.0, 256.0, false);
                vec![PlacedModel {
                    name: model_name.to_string(),
                    mesh: merged,
                    transform,
                    settings: vec![("ironing_enabled".into(), "false".into())],
                    slot: None,
                }]
            } else {
                let transform = mesh::place_on_bed(mesh, 256.0, 256.0, false);
                vec![PlacedModel {
                    name: model_name.to_string(),
                    mesh: mesh.clone(),
                    transform,
                    settings: vec![("ironing_enabled".into(), "false".into())],
                    slot: None,
                }]
            };
            let mut base_settings = UserSettings {
                layer_height: Some(0.2),
                material_bed_temperature: Some(60.0),
                material_print_temperature: Some(220.0),
                extra: vec![],
                ..UserSettings::default()
            };
            base_settings.nonplanar = NonplanarOptions {
                mode: NonplanarMode::CurvedTops,
                ..NonplanarOptions::default()
            };
            let tag = format!("{printer_id}-{model_name}");
            let planar_path = std::env::temp_dir().join(format!(
                "shale-np-{tag}-planar-{}.gcode",
                std::process::id()
            ));
            let curved_path = std::env::temp_dir().join(format!(
                "shale-np-{tag}-curved-{}.gcode",
                std::process::id()
            ));
            let planar_out = job::slice(
                &catalog,
                &engine,
                printer_id,
                Some(VARIANT.into()),
                Some(quality.clone()),
                &[Some(MATERIAL.into())],
                UserSettings {
                    nonplanar: NonplanarOptions::default(),
                    ..base_settings.clone()
                },
                &placed,
                &[],
                None,
                &[],
                planar_path,
                &mut |_| {},
            )
            .expect("planar slice");
            let curved_out = if *model_name == "pillar-beside-dome" {
                let (gcode, report) =
                    slice_pillar_beside_dome_sequential(&catalog, &engine, printer_id, &tag);
                std::fs::write(&curved_path, &gcode).expect("curved gcode");
                SliceOutcome {
                    gcode_path: curved_path.clone(),
                    resolved: planar_out.resolved.clone(),
                    toolpath: shale_core::gcode::parse_toolpaths(&gcode),
                    print_time_seconds: None,
                    filament_metres: 0.0,
                    flavour: planar_out.flavour,
                    log_tail: String::new(),
                    machine_gcode: planar_out.machine_gcode,
                    nonplanar_report: Some(report),
                    engine_gcode: None,
                    planar_baseline_gcode: None,
                    nonplanar_context: None,
                }
            } else {
                job::slice(
                    &catalog,
                    &engine,
                    printer_id,
                    Some(VARIANT.into()),
                    Some(quality.clone()),
                    &[Some(MATERIAL.into())],
                    base_settings,
                    &placed,
                    &[],
                    None,
                    &[],
                    curved_path,
                    &mut |_| {},
                )
                .expect("curved slice")
            };
            let report = curved_out.nonplanar_report.as_ref().expect("report");
            assert!(
                report.safety.is_some() || report.fallback_to_planar,
                "{printer_id}/{model_name}: no safety report and no fallback"
            );
            // A machine that cannot be verified on a printer falls back to the planar
            // baseline; that is a recorded outcome, not a failure of the matrix.
            let safety = report.safety.as_ref();
            rows.push(MatrixRow {
                printer_id: printer_id.to_string(),
                model: model_name.to_string(),
                quality: quality.clone(),
                planar_time_s: planar_out.print_time_seconds,
                curved_time_s: curved_out.print_time_seconds,
                collision_fixes: report.collision_adjustments.len(),
                travel_collisions_remaining: safety
                    .map(|s| s.travel_collisions_remaining)
                    .unwrap_or(0),
                planar_equivalence_ok: report.planar_equivalence_ok,
                safety_ok: safety.map(|s| s.ok).unwrap_or(false),
                max_slope_deg: safety.and_then(|s| s.max_extrusion_slope_deg),
                safety_summary: report.fallback_reasons.first().cloned().unwrap_or_else(|| {
                    safety
                        .map(safety_summary)
                        .unwrap_or_else(|| "no safety report".into())
                }),
            });
            if report.fallback_to_planar {
                assert!(
                    !report.fallback_reasons.is_empty(),
                    "{printer_id}/{model_name}: fallback without a reason"
                );
                assert_eq!(
                    report.collision_adjustments.len(),
                    0,
                    "{printer_id}/{model_name}: a fallback needs no field surgery"
                );
                continue;
            }
            let safety = safety.expect("a kept curved slice carries its safety report");
            if *model_name != "pillar-beside-dome" {
                assert!(
                    report.planar_equivalence_ok,
                    "{printer_id}/{model_name}: planar equivalence failed"
                );
            }
            assert!(
                safety.ok,
                "{printer_id}/{model_name}: safety {:?}",
                safety.violations
            );
            if let Some(slope) = safety.max_extrusion_slope_deg {
                assert!(
                    slope <= safety.slope_cap_deg + 1e-3,
                    "{printer_id}/{model_name}: slope {slope:.3}° > cap {:.3}°",
                    safety.slope_cap_deg
                );
            }
            if *model_name == "pillar-beside-dome" {
                // The field is never edited after slicing: a kept curved slice must
                // already be clear of printed material.
                assert_eq!(
                    report.collision_adjustments.len(),
                    0,
                    "{printer_id}: the post-pass must not edit the field after slicing"
                );
                assert_eq!(
                    safety.travel_collisions_remaining, 0,
                    "{printer_id}: travel collisions must be zero in free final G-code"
                );
            }
        }
    }
    let json = serde_json::to_string_pretty(&rows).expect("json");
    std::fs::write(artifact_dir().join("printer-matrix.json"), json).expect("write");
}