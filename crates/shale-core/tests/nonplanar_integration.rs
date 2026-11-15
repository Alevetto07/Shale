//! End-to-end non-planar curved tops vs planar baseline with CuraEngine.
//!
//! Writes before/after G-code and a matplotlib PNG when resources are present.

use std::path::PathBuf;

use shale_core::engine::CliEngine;
use shale_core::job::{self, PlacedModel};
use shale_core::mesh;
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
}

fn artifact_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/nonplanar");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn slice_sphere(nonplanar: bool, tag: &str) -> (String, job::SliceOutcome) {
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available());
    let mesh = primitives::sphere(12.0);
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: "dome".into(),
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
    let dest = artifact_dir().join(format!("{tag}.gcode"));
    let _ = std::fs::write(&dest, &text);
    (text, outcome)
}

#[test]
fn planar_and_curved_dome_slice_with_cura() {
    if !configured() {
        return;
    }
    let (planar, planar_out) = slice_sphere(false, "dome-planar");
    let (curved, curved_out) = slice_sphere(true, "dome-curved");

    assert!(planar_out.nonplanar_report.is_none());
    assert!(curved_out.nonplanar_report.is_some());

    let (_min, max_var) = shale_nonplanar::top_layer_z_variation(&curved)
        .expect("curved gcode should have a top layer");
    let (pmin, pmax) = shale_nonplanar::top_layer_z_variation(&planar)
        .expect("planar gcode should have a top layer");
    let planar_spread = (pmax - pmin).abs();
    let curved_spread = (max_var - _min).abs();
    assert!(
        curved_spread > planar_spread + 0.02,
        "curved top should vary in Z more than planar (curved={curved_spread:.3} planar={planar_spread:.3})"
    );

    assert!(shale_nonplanar::gcode_respects_envelope(&curved, 10.0));

    if let Some(planar_t) = planar_out.print_time_seconds {
        if let Some(curved_t) = curved_out.print_time_seconds {
            let _ = std::fs::write(
                artifact_dir().join("times.txt"),
                format!(
                    "planar_seconds={planar_t:.1}\ncurved_seconds={curved_t:.1}\n"
                ),
            );
        }
    }

    let plot = artifact_dir().join("dome-curved-toplayer.png");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/plot-nonplanar-toplayers.py");
    if script.is_file() {
        let _ = std::process::Command::new("python3")
            .arg(&script)
            .arg(artifact_dir().join("dome-curved.gcode"))
            .arg("-o")
            .arg(&plot)
            .arg("--title")
            .arg("Dome curved tops — top layer Z")
            .status();
    }
}

/// Chevron ramp ~12° — within the 10° limit after smoothing, should still slice.
#[test]
fn chevron_ramp_slices_nonplanar() {
    if !configured() {
        return;
    }
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    let wing_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/t-wing.stl");
    let mesh = if wing_path.is_file() {
        mesh::load_mesh(&wing_path).expect("wing stl")
    } else {
        primitives::sphere(8.0)
    };
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: "wing".into(),
        mesh,
        transform,
        settings: Vec::new(),
        slot: None,
    }];
    let mut settings = UserSettings {
        layer_height: Some(0.2),
        ..UserSettings::default()
    };
    settings.nonplanar.mode = NonplanarMode::CurvedTops;
    let path = std::env::temp_dir().join("shale").join("nonplanar-wing.gcode");
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
        path,
        &mut |_| {},
    );
    assert!(outcome.is_ok());
}