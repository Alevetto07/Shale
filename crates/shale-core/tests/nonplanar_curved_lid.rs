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
/// Quality process id per printer: Bambu's catalogue names them after the
/// machine, so the X1 Carbon's 0.20 mm standard is not the A1's.
fn quality_for(printer: &str) -> String {
    match printer {
        "bambu_lab_x1_carbon" => "0_20mm_standard_bbl_x1c".into(),
        "bambu_lab_a1_mini" => "0_20mm_standard_bbl_a1m".into(),
        "bambu_lab_p1s" => "0_20mm_standard_bbl_p1s".into(),
        _ => "0_20mm_standard_bbl_a1".into(),
    }
}
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
        mesh::load_mesh(&demo_lid()).expect("load examples/nonplanar-demo.stl");
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
    let path = std::env::temp_dir().join("shale").join(format!(
        "nonplanar-curved-lid-{tag}-{}.gcode",
        std::process::id()
    ));
    let outcome = job::slice(
        &catalog,
        &engine,
        printer,
        Some(VARIANT.into()),
        Some(quality_for(printer)),
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

/// Extruding into XY where the file already put material more than
/// [`INVERSION_DROP_MM`] higher: the nozzle digs back into the part it is
/// building. This is the repo's own definition (see
/// `nonplanar_z_inversions.rs`); counting descending Z along a tilted level
/// surface would flag every legitimate contour, because a sheared layer rises
/// and falls as it goes round the part.
fn z_inversions(gcode: &str) -> usize {
    use std::collections::HashMap;
    const CELL_MM: f32 = 0.4;
    const SAMPLE_MM: f32 = 0.25;
    const INVERSION_DROP_MM: f32 = 0.1;
    let toolpath = shale_core::gcode::parse_toolpaths(gcode);
    let mut printed: HashMap<(i32, i32), f32> = HashMap::new();
    let mut hits = 0usize;
    let cell = |x: f32, y: f32| ((x / CELL_MM).floor() as i32, (y / CELL_MM).floor() as i32);
    for layer in &toolpath.layers {
        for poly in &layer.polylines {
            if poly.kind == "travel" || poly.points.len() < 2 {
                continue;
            }
            for pair in poly.points.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let len = (dx * dx + dy * dy).sqrt();
                let steps = ((len / SAMPLE_MM).ceil() as usize).max(1);
                for s in 0..=steps {
                    let t = s as f32 / steps as f32;
                    let x = a[0] + dx * t;
                    let y = a[1] + dy * t;
                    let z = a[2] + (b[2] - a[2]) * t;
                    let seen = printed.entry(cell(x, y)).or_insert(f32::NEG_INFINITY);
                    if z < *seen - INVERSION_DROP_MM {
                        hits += 1;
                    } else if z > *seen {
                        *seen = z;
                    }
                }
            }
        }
    }
    hits
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
    assert_eq!(
        safety.collisions_extruding, 0,
        "{printer} extruding collisions"
    );
    assert_eq!(safety.collisions_travel, 0, "{printer} travel collisions");
    let slope = safety.max_extrusion_slope_deg.unwrap_or(0.0);
    assert!(
        slope <= 10.5,
        "{printer}: toolhead would follow {slope:.2} deg (cap 10 + 0.5)"
    );
    assert_eq!(
        z_inversions(&gcode),
        0,
        "{printer}: Z inversions in the band"
    );
}

/// A constant deformed layer must stay inside the cap between nodes as well
/// as on them. Interpolating the ramp parameters used to leave a 75° spike in
/// the middle of a cell the node scan called flat.
#[test]
fn demo_lid_level_surfaces_stay_inside_the_cap_between_nodes() {
    let mesh: TriangleMesh = mesh::load_mesh(&demo_lid()).expect("stl");
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let baked = mesh::bake(&mesh, &transform);
    let m = shale_nonplanar::mesh::Mesh {
        vertices: baked
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: baked.triangles.clone(),
    };
    let opts = NonplanarOptions {
        mode: NonplanarMode::Auto,
        max_angle_deg: 10.0,
        grid_step_mm: 0.35,
        ..NonplanarOptions::default()
    };
    let ctx = shale_nonplanar::pipeline::plan_field(&m, &opts, 0.2, &[]);
    assert!(
        ctx.planar_collapsed.is_none(),
        "the lid must stay curved: {:?}",
        ctx.planar_collapsed
    );
    let field = &ctx.field;
    let thick = shale_nonplanar::safety::physical_layer_thickness(field);
    assert_eq!(
        thick.violations, 0,
        "layer thickness {:.3}..{:.3} mm",
        thick.min_mm, thick.max_mm
    );
    let step = field.grid.step;
    let mut worst = 0.0_f64;
    let mut worst_at = String::new();
    for iy in 0..field.grid.height.saturating_sub(1) {
        for ix in 0..field.grid.width.saturating_sub(1) {
            let x = field.grid.origin[0] + (ix as f64 + 0.5) * step;
            let y = field.grid.origin[1] + (iy as f64 + 0.5) * step;
            let lift = field.lift_at(x, y);
            let lift_x = field.lift_at(x + step, y);
            let lift_y = field.lift_at(x, y + step);
            if lift.abs() < 0.2 || lift_x.abs() < 0.2 || lift_y.abs() < 0.2 {
                continue;
            }
            for &z_def in &[4.0_f64, 5.2, 8.0, 11.0, 13.5] {
                let z = field.inverse_z(x, y, z_def);
                let zx = field.inverse_z(x + step, y, z_def);
                let zy = field.inverse_z(x, y + step, z_def);
                let sx = ((zx - z) / step).atan().to_degrees().abs();
                let sy = ((zy - z) / step).atan().to_degrees().abs();
                let slope = sx.max(sy);
                if slope > worst {
                    worst = slope;
                    worst_at = format!("({x:.2},{y:.2}) zdef={z_def:.1}");
                }
            }
        }
    }
    assert!(
        worst <= 10.5,
        "a level between nodes reaches {worst:.2}° at {worst_at}, past the 10° cap"
    );
    let mut round_trip = 0.0_f64;
    for iy in (0..field.grid.height).step_by(8) {
        for ix in (0..field.grid.width).step_by(8) {
            let x = field.grid.origin[0] + ix as f64 * step;
            let y = field.grid.origin[1] + iy as f64 * step;
            if field.lift_at(x, y).abs() < 0.2 {
                continue;
            }
            for z in [2.0_f64, 5.2, 8.0, 11.0, 14.0, 16.0] {
                let mapped = field.forward_z(x, y, z);
                let back = field.inverse_z(x, y, mapped);
                round_trip = round_trip.max((back - z).abs());
            }
        }
    }
    assert!(
        round_trip < 0.01,
        "forward/inverse round trip misses by {round_trip:.3} mm"
    );
    let min_lift = (0..field.island_strength.len())
        .map(|i| field.node_lift(i))
        .fold(0.0_f64, f64::min);
    assert!(
        min_lift < -2.0,
        "the peak should still be pressed down, lift {min_lift:.3}"
    );
}

#[test]
fn demo_lid_curves_on_a1() {
    assert_curved_lid("bambu_lab_a1", "lid-a1");
}

#[test]
fn demo_lid_curves_on_x1_carbon() {
    assert_curved_lid("bambu_lab_x1_carbon", "lid-x1c");
}
