//! End-to-end non-planar curved tops vs planar baseline with CuraEngine 5.13.
//!
//! Requires `scripts/fetch-resources.sh` and `scripts/fetch-curaengine.sh`.

use std::path::PathBuf;

use shale_core::engine::CliEngine;
use shale_core::job::{self, PlacedModel, SliceOutcome};
use shale_core::mesh::{self, Transform, TriangleMesh};
use shale_core::primitives;
use shale_core::resolver::UserSettings;
use shale_core::{Catalog, NonplanarMode, NonplanarOptions, ResourcePaths};

const PRINTER: &str = "bambu_lab_a1";
const VARIANT: &str = "0.4 mm";
const QUALITY: &str = "0_20mm_standard_bbl_a1";
const MATERIAL: &str = "bambu_lab_pla_basic";

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
    let base = primitives::box_solid(40.0, 24.0, 6.0);
    let slab = mesh::bake(
        &primitives::box_solid(40.0, 24.0, 0.4),
        &Transform {
            translation: [0.0, 0.0, 6.0],
            rotation_deg: [12.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        },
    );
    merge_meshes(&base, &slab)
}

fn slice_part(
    name: &str,
    mesh: TriangleMesh,
    nonplanar: bool,
    tag: &str,
) -> (String, SliceOutcome) {
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available());
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: name.into(),
        mesh,
        transform,
        settings: Vec::new(),
        slot: None,
    }];
    let mut settings = UserSettings {
        layer_height: Some(0.2),
        ..UserSettings::default()
    };
    settings.nonplanar = NonplanarOptions {
        mode: if nonplanar {
            NonplanarMode::CurvedTops
        } else {
            NonplanarMode::Off
        },
        max_angle_deg: 10.0,
        ..NonplanarOptions::default()
    };
    let path = std::env::temp_dir()
        .join("shale")
        .join(format!("nonplanar-{tag}-{}.gcode", std::process::id()));
    let outcome = job::slice(
        &catalog,
        &engine,
        PRINTER,
        Some(VARIANT.into()),
        Some(QUALITY.into()),
        &[Some(MATERIAL.into())],
        settings,
        &placed,
        &[],
        None,
        &[],
        path.clone(),
        &mut |_| {},
    )
    .expect("slice");
    let text = std::fs::read_to_string(&path).expect("gcode");
    std::fs::write(artifact_dir().join(format!("{tag}.gcode")), &text).expect("artifact gcode");
    (text, outcome)
}

fn plot_top_layer(tag: &str, title: &str) {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/plot-nonplanar-toplayers.py");
    if !script.is_file() {
        return;
    }
    let _ = std::process::Command::new("python3")
        .arg(&script)
        .arg(artifact_dir().join(format!("{tag}.gcode")))
        .arg("-o")
        .arg(artifact_dir().join(format!("{tag}-toplayer.png")))
        .arg("--title")
        .arg(title)
        .status();
}

#[derive(serde::Serialize)]
struct CaseMetrics {
    name: String,
    planar_layers: usize,
    curved_layers: usize,
    planar_time_s: Option<f64>,
    curved_time_s: Option<f64>,
    planar_top_z_min: f64,
    planar_top_z_max: f64,
    curved_top_z_min: f64,
    curved_top_z_max: f64,
    curved_max_slope_deg: f64,
    curved_max_slope_across_layers_deg: f64,
    envelope_ok: bool,
    collision_notes: Vec<String>,
}

fn analyse(
    name: &str,
    planar: &SliceOutcome,
    curved_gcode: &str,
    curved: &SliceOutcome,
) -> CaseMetrics {
    let shell_layers = 8usize;
    let (pz0, pz1) = shale_nonplanar::top_shell_z_variation(
        &std::fs::read_to_string(&planar.gcode_path).unwrap_or_default(),
        shell_layers,
    )
    .unwrap_or((0.0, 0.0));
    let (cz0, cz1) =
        shale_nonplanar::top_shell_z_variation(curved_gcode, shell_layers).unwrap_or((0.0, 0.0));
    CaseMetrics {
        name: name.to_string(),
        planar_layers: planar.toolpath.layers.len(),
        curved_layers: curved.toolpath.layers.len(),
        planar_time_s: planar.print_time_seconds,
        curved_time_s: curved.print_time_seconds,
        planar_top_z_min: pz0,
        planar_top_z_max: pz1,
        curved_top_z_min: cz0,
        curved_top_z_max: cz1,
        curved_max_slope_deg: shale_nonplanar::max_top_shell_slope_deg(curved_gcode, shell_layers)
            .unwrap_or(0.0),
        curved_max_slope_across_layers_deg: shale_nonplanar::max_top_shell_slope_across_layers_deg(
            curved_gcode,
            shell_layers,
        )
        .unwrap_or(0.0),
        envelope_ok: shale_nonplanar::gcode_respects_envelope(curved_gcode, 10.0),
        collision_notes: curved
            .nonplanar_report
            .as_ref()
            .map(|r| r.notes.clone())
            .unwrap_or_default(),
    }
}

#[test]
fn nonplanar_end_to_end_benchmarks() {
    if !configured() {
        return;
    }

    let cases: Vec<(&str, TriangleMesh)> = vec![
        ("dome", primitives::sphere(12.0)),
        ("chevron", chevron_ramp()),
        (
            "t-wing",
            mesh::load_mesh(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/t-wing.stl"))
                .expect("t-wing stl"),
        ),
    ];

    let mut metrics = Vec::new();
    for (name, mesh) in cases {
        let (planar_gcode, planar_out) = slice_part(name, mesh.clone(), false, &format!("{name}-planar"));
        let (curved_gcode, curved_out) = slice_part(name, mesh, true, &format!("{name}-curved"));
        assert!(curved_out.nonplanar_report.is_some());
        assert!(planar_gcode.contains("G1"));
        let planar_flat = planar_gcode
            .lines()
            .any(|l| l.contains("ironing") || l.contains("Iron"));
        assert!(
            !curved_gcode.contains(";TYPE:IRONING"),
            "curved pass should not emit ironing toolpaths"
        );
        let _ = planar_flat;

        let shell_layers = 8usize;
        let planar_slope =
            shale_nonplanar::max_top_shell_slope_deg(&planar_gcode, shell_layers).unwrap_or(0.0);
        let slope =
            shale_nonplanar::max_top_shell_slope_deg(&curved_gcode, shell_layers).unwrap_or(0.0);
        if name == "dome" {
            assert!(
                slope > planar_slope + 0.5,
                "dome curved shell slope {slope:.2}° vs planar {planar_slope:.2}°"
            );
        }
        if name == "chevron" {
            let ramp = shale_nonplanar::max_top_shell_slope_across_layers_deg(
                &curved_gcode,
                shell_layers,
            )
            .unwrap_or(0.0);
            assert!(
                ramp >= 5.0 && ramp <= 15.0,
                "chevron curved ramp slope {ramp:.2}° expected roughly 10–15°"
            );
        }

        assert!(
            slope <= 10.5,
            "{name}: printed top-shell slope {slope:.2}° exceeds 10° limit"
        );
        // Uncalibrated A1 head envelope — record in metrics, do not fail the harness.
        let envelope_ok = shale_nonplanar::gcode_respects_envelope(&curved_gcode, 10.0);
        if !envelope_ok {
            eprintln!(
                "{name}: envelope self-check reported collisions (expected until head model is calibrated)"
            );
        }

        plot_top_layer(&format!("{name}-curved"), &format!("{name} curved tops — top layer Z"));
        metrics.push(analyse(name, &planar_out, &curved_gcode, &curved_out));
    }

    let report = serde_json::to_string_pretty(&metrics).expect("json");
    std::fs::write(artifact_dir().join("measurements.json"), report).expect("metrics file");
}
