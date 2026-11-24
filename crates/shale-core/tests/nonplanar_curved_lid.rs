// SPDX-License-Identifier: AGPL-3.0-or-later
//! The curved top has to survive the end-to-end pass, not just the field.
//!
//! `examples/nonplanar-demo.stl` is an 80 x 50 mm lid whose analytic top slope is
//! 6.04 deg. It used to fall back on every Bambu printer: the band was *rescaled*
//! instead of sheared, the slicer laid ~50 layers into 1.6 mm of plastic, and the
//! sweep found thousands of travel moves ploughing through the bead they had just
//! laid. These tests pin the outcome rather than the mechanism: the file must be
//! curved, the curved moves must be substantial, and the head must stay clear.

mod nonplanar_fixtures;

use std::path::PathBuf;

use shale_core::engine::CliEngine;
use shale_core::job::{self, PlacedModel, SliceOutcome};
use shale_core::mesh::{self, TriangleMesh};
use shale_core::resolver::UserSettings;
use shale_core::{Catalog, NonplanarMode, NonplanarOptions, ResourcePaths};

const VARIANT: &str = "0.4 mm";
const QUALITY: &str = "0_20mm_standard_bbl_a1";
const MATERIAL: &str = "bambu_lab_pla_basic";

fn configured() -> bool {
    std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some()
        && std::env::var_os("SHALE_ORCA_DIR").is_some()
}

fn demo_lid() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/nonplanar-demo.stl")
}

fn slice_lid(printer: &str, tag: &str) -> (String, SliceOutcome) {
    let mesh: TriangleMesh =
        mesh::load_mesh(&demo_lid(), 1.0).expect("load examples/nonplanar-demo.stl");
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: "nonplanar-demo".into(),
        mesh,
        transform,
        settings: vec![("ironing_enabled".into(), "false".into())],
        slot: None,
    }];
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available());
    let mut settings = UserSettings {
        layer_height: Some(0.2),
        ..UserSettings::default()
    };
    settings.nonplanar = NonplanarOptions {
        mode: NonplanarMode::Auto,
        max_angle_deg: 10.0,
        ..NonplanarOptions::default()
    };
    let path = std::env::temp_dir()
        .join("shale")
        .join(format!("nonplanar-curved-lid-{tag}-{}.gcode", std::process::id()));
    let outcome = job::slice(
        &catalog,
        &engine,
        printer,
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
    (text, outcome)
}

/// Extruding moves whose Z changes along the move: the sweep that makes a top
/// curved. A planar file only carries Z on the first move of each layer, so its
/// count is a handful per layer at most.
fn curved_extruding_moves(gcode: &str) -> usize {
    let mut pos = [0.0f64; 3];
    let mut count = 0usize;
    for line in gcode.lines() {
        let line = line.trim();
        if !line.starts_with("G1") || !line.contains('E') {
            continue;
        }
        let mut next = pos;
        for (i, word) in line.split_whitespace().enumerate() {
            if i == 0 {
                continue;
            }
            let mut chars = word.chars();
            let axis = chars.next().unwrap_or(' ');
            let value: f64 = chars.as_str().parse().unwrap_or(f64::NAN);
            match axis {
                'X' => next[0] = value,
                'Y' => next[1] = value,
                'Z' => next[2] = value,
                _ => {}
            }
        }
        let horizontal = ((next[0] - pos[0]).powi(2) + (next[1] - pos[1]).powi(2)).sqrt();
        if horizontal > 0.05 && (next[2] - pos[2]).abs() > 0.002 {
            count += 1;
        }
        pos = next;
    }
    count
}

/// Z decreases along the toolpath inside one layer: the head would dig back into
/// material it has just laid on the same level surface.
fn z_inversions(gcode: &str) -> usize {
    let mut pos_z = 0.0f64;
    let mut high_water = 0.0f64;
    let mut layer_started = false;
    let mut inversions = 0usize;
    for line in gcode.lines() {
        let line = line.trim();
        if line.starts_with(";LAYER:") {
            layer_started = true;
            high_water = 0.0;
            continue;
        }
        if !layer_started || !line.starts_with("G1") || !line.contains('E') {
            continue;
        }
        if let Some(idx) = line.find('Z') {
            let rest = &line[idx + 1..];
            let end = rest
                .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                .unwrap_or(rest.len());
            if let Ok(z) = rest[..end].parse::<f64>() {
                pos_z = z;
            }
        }
        if pos_z > high_water + 0.002 {
            high_water = pos_z;
        } else if pos_z < high_water - 0.05 {
            inversions += 1;
        }
    }
    inversions
}

fn assert_curved_lid(printer: &str, tag: &str) {
    if !configured() {
        eprintln!("skipping {tag}: engine not configured");
        return;
    }
    let (gcode, outcome) = slice_lid(printer, tag);
    let report = outcome.nonplanar_report.expect("nonplanar report");
    assert!(
        !report.fallback_to_planar,
        "{printer} fell back: {:?}",
        report.fallback_reasons
    );
    let curved = curved_extruding_moves(&gcode);
    assert!(
        curved > 2000,
        "{printer}: only {curved} curved extruding moves - the top is not being swept"
    );
    let safety = report.safety.as_ref().expect("safety report");
    assert_eq!(safety.collisions_extruding, 0, "{printer} extruding collisions");
    assert_eq!(safety.collisions_travel, 0, "{printer} travel collisions");
    let slope = safety.max_extrusion_slope_deg.unwrap_or(0.0);
    assert!(
        slope <= 10.5,
        "{printer}: toolhead would follow {slope:.2} deg (cap 10 + 0.5)"
    );
    assert_eq!(z_inversions(&gcode), 0, "{printer}: Z inversions in the band");
}

#[test]
fn demo_lid_curves_on_a1() {
    assert_curved_lid("bambu_lab_a1", "lid-a1");
}

#[test]
fn demo_lid_curves_on_x1_carbon() {
    assert_curved_lid("bambu_lab_x1_carbon", "lid-x1c");
}
