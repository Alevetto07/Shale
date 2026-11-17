//! Curved-top safety smoke: dome, chevron slab, and t-wing on every enveloped Bambu printer.

use std::path::PathBuf;

use shale_core::job::{self, PlacedModel};
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
            let transform = mesh::place_on_bed(mesh, 256.0, 256.0, false);
            let placed = vec![PlacedModel {
                name: model_name.to_string(),
                mesh: mesh.clone(),
                transform,
                settings: vec![("ironing_enabled".into(), "false".into())],
                slot: None,
            }];
            let mut base_settings = UserSettings {
                layer_height: Some(0.2),
                material_bed_temperature: Some(60.0),
                material_print_temperature: Some(220.0),
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
            let curved_out = job::slice(
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
            .expect("curved slice");
            let report = curved_out.nonplanar_report.as_ref().expect("report");
            let safety = report.safety.as_ref().expect("safety");
            rows.push(MatrixRow {
                printer_id: printer_id.to_string(),
                model: model_name.to_string(),
                quality: quality.clone(),
                planar_time_s: planar_out.print_time_seconds,
                curved_time_s: curved_out.print_time_seconds,
                collision_fixes: report.collision_adjustments.len(),
                travel_collisions_remaining: safety.travel_collisions_remaining,
                planar_equivalence_ok: report.planar_equivalence_ok,
                safety_ok: safety.ok,
                max_slope_deg: safety.max_extrusion_slope_deg,
                safety_summary: safety_summary(safety),
            });
            assert!(
                report.planar_equivalence_ok,
                "{printer_id}/{model_name}: planar equivalence failed"
            );
            assert!(
                safety.ok,
                "{printer_id}/{model_name}: safety {:?}",
                safety.violations
            );
        }
    }
    let json = serde_json::to_string_pretty(&rows).expect("json");
    std::fs::write(artifact_dir().join("printer-matrix.json"), json).expect("write");
}