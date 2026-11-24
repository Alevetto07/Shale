// SPDX-License-Identifier: AGPL-3.0-or-later
//! A curved slice may never deposit layers outside the slicer's own window.
//!
//! The inverse map sends every deformed layer to a physical thickness
//! `nominal * dz_phys/dz_def`. Through a column's transition that ratio is
//! `ramp / (ramp + lift)`, so a fixed 0.4 mm ramp under a multi-millimetre lift
//! used to put twenty-odd deformed layers into 0.4 mm of plastic: 0.018 mm
//! physical layers, with `extrusion_scale` clamped at 0.5 where it needed 0.045,
//! so the transition printed several times the plastic the layer had room for.
//! Nothing rejected the file.
//!
//! This test measures the thickness the printer will actually see, from the
//! G-code alone: for every XY cell it compares the mean Z of consecutive
//! deposited layers. Cells whose layer is not locally flat are skipped, because
//! a layer that climbs across the cell (up to `tan(10 deg) * 0.4 mm = 0.07 mm`)
//! cannot be measured to better than that, and a measurement noise larger than
//! `min_layer` proves nothing either way.
//!
//! The strict guarantee is the field check in the safety report
//! (`min_physical_layer_mm` / `max_physical_layer_mm`, computed from the map
//! itself with no measurement noise); the G-code scan is the end-to-end proof
//! that the file carries the field's intent. A job may also fall back to planar,
//! but only with a named reason — silently crushing the transition is the bug.
//!
//! Fixtures: `examples/nonplanar-demo.stl` (the lid, whose island spans 8.4 mm of
//! top height) and the 12 mm dome (`primitives::sphere(12.0)`).

use std::collections::HashMap;
use std::path::PathBuf;

use shale_core::engine::CliEngine;
use shale_core::job::{self, PlacedModel};
use shale_core::mesh::{self, TriangleMesh};
use shale_core::primitives;
use shale_core::resolver::UserSettings;
use shale_core::{Catalog, NonplanarMode, NonplanarOptions, NonplanarReport, ResourcePaths};

const PRINTER: &str = "bambu_lab_a1";
const VARIANT: &str = "0.4 mm";
const QUALITY: &str = "0_20mm_standard_bbl_a1";
const MATERIAL: &str = "bambu_lab_pla_basic";

/// Slicer layer height, and the window a physical layer has to land in.
const NOMINAL: f32 = 0.2;
const MIN_LAYER: f32 = 0.08;
const MAX_LAYER: f32 = 0.28;
/// XY lattice the per-layer means are kept on, in millimetres.
const CELL_MM: f32 = 0.4;
/// Slack on the G-code measurement.
///
/// A cell whose layer spans at most 0.05 mm reads its mean to about ±0.02 mm;
/// the field check carries the exact bound, this one only has to catch a layer
/// crushed to a fraction of `min_layer`.
const MEASURE_TOL: f32 = 0.02;
/// Largest within-cell Z span for a layer to be measurable, in millimetres.
const FLAT_MM: f32 = 0.05;

fn configured() -> bool {
    std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some()
        && std::env::var_os("SHALE_ORCA_DIR").is_some()
}

fn example(name: &str) -> TriangleMesh {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    mesh::load_stl(&bytes).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()))
}

fn slice(mesh: &TriangleMesh, tag: &str) -> (String, NonplanarReport) {
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available(), "CuraEngine not available");
    let mut settings = UserSettings {
        layer_height: Some(NOMINAL as f64),
        ..UserSettings::default()
    };
    settings.nonplanar = NonplanarOptions {
        mode: NonplanarMode::Auto,
        max_angle_deg: 10.0,
        grid_step_mm: 0.35,
        ..NonplanarOptions::default()
    };
    let transform = mesh::place_on_bed(mesh, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: tag.into(),
        mesh: mesh.clone(),
        transform,
        settings: vec![("ironing_enabled".into(), "false".into())],
        slot: None,
    }];
    let path = std::env::temp_dir()
        .join("shale")
        .join(format!("thickness-{tag}-{}.gcode", std::process::id()));
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
    let gcode = std::fs::read_to_string(path).expect("gcode");
    let report = outcome
        .nonplanar_report
        .unwrap_or_else(|| panic!("{tag}: no non-planar report"));
    (gcode, report)
}

/// The `;SHALE_NONPLANAR` header line, if the job wrote one.
fn header(gcode: &str) -> Option<&str> {
    gcode
        .lines()
        .find(|line| line.starts_with(";SHALE_NONPLANAR"))
}

/// Per-cell physical layer thickness between consecutive deposited layers.
#[derive(Default)]
struct ThicknessScan {
    pairs: usize,
    min_mm: f32,
    max_mm: f32,
    violations: Vec<String>,
}

/// `(cell, layer) -> (lowest z, highest z, sum of z, sample count)`.
type CellLayers = HashMap<((i32, i32), usize), (f32, f32, f32, usize)>;
/// `cell -> [(layer, mean z, z span)]`.
type CellProfile = HashMap<(i32, i32), Vec<(usize, f32, f32)>>;

fn scan_thickness(gcode: &str) -> ThicknessScan {
    let toolpath = shale_core::gcode::parse_toolpaths(gcode);
    let mut cell_layers: CellLayers = HashMap::new();
    let cell = |x: f32, y: f32| ((x / CELL_MM).floor() as i32, (y / CELL_MM).floor() as i32);
    for (index, layer) in toolpath.layers.iter().enumerate() {
        for poly in &layer.polylines {
            if poly.kind == "travel" {
                continue;
            }
            for point in &poly.points {
                let key = (cell(point[0], point[1]), index);
                let entry = cell_layers
                    .entry(key)
                    .or_insert((f32::INFINITY, f32::NEG_INFINITY, 0.0, 0));
                entry.0 = entry.0.min(point[2]);
                entry.1 = entry.1.max(point[2]);
                entry.2 += point[2];
                entry.3 += 1;
            }
        }
    }
    // cell -> [(layer, mean z, span)]
    let mut by_cell: CellProfile = HashMap::new();
    for ((xy, index), (lo, hi, sum, count)) in cell_layers {
        by_cell
            .entry(xy)
            .or_default()
            .push((index, sum / count as f32, hi - lo));
    }
    let mut scan = ThicknessScan {
        min_mm: f32::INFINITY,
        max_mm: f32::NEG_INFINITY,
        ..Default::default()
    };
    for ((x, y), mut layers) in by_cell {
        layers.sort_by_key(|(index, _, _)| *index);
        for pair in layers.windows(2) {
            let (ia, za, spread_a) = pair[0];
            let (ib, zb, spread_b) = pair[1];
            if ib != ia + 1 || spread_a > FLAT_MM || spread_b > FLAT_MM {
                continue;
            }
            let thickness = zb - za;
            if thickness <= 0.0 {
                // A descending map is the z-inversion test's job.
                continue;
            }
            scan.pairs += 1;
            scan.min_mm = scan.min_mm.min(thickness);
            scan.max_mm = scan.max_mm.max(thickness);
            if thickness < MIN_LAYER - MEASURE_TOL || thickness > MAX_LAYER + MEASURE_TOL {
                if scan.violations.len() < 5 {
                    scan.violations.push(format!(
                        "cell ({x},{y}) layers {ia}->{ib}: {thickness:.3} mm"
                    ));
                } else {
                    scan.violations.push(String::new());
                }
            }
        }
    }
    scan
}

fn check(name: &str, gcode: &str, report: &NonplanarReport) {
    if let Some(line) = header(gcode) {
        eprintln!("{name}: {line}");
    }
    let safety = report
        .safety
        .as_ref()
        .unwrap_or_else(|| panic!("{name}: no safety report"));
    if let (Some(min), Some(max)) = (safety.min_physical_layer_mm, safety.max_physical_layer_mm) {
        eprintln!(
            "{name}: field deposits {min:.3}-{max:.3} mm layers over {} samples (window [{MIN_LAYER}, {MAX_LAYER}])",
            safety.layer_thickness_samples
        );
        assert!(
            min >= MIN_LAYER as f64 - 0.005 && max <= MAX_LAYER as f64 + 0.005,
            "{name}: field deposits {min:.3}-{max:.3} mm layers, window is [{MIN_LAYER}, {MAX_LAYER}]"
        );
    }
    if report.fallback_to_planar {
        // A fallback is allowed, silence is not: the reasons have to name what
        // the file could not do, and they must never paper over a transition
        // that deposits layers outside the window.
        assert!(
            !report.fallback_reasons.is_empty(),
            "{name}: fell back without a reason"
        );
        assert!(
            safety.layer_thickness_within_bounds
                || report
                    .fallback_reasons
                    .iter()
                    .any(|r| r.contains("layer thickness")),
            "{name}: fell back on {:?} while the field deposits {:?}-{:?} mm layers",
            report.fallback_reasons,
            safety.min_physical_layer_mm,
            safety.max_physical_layer_mm
        );
        eprintln!("{name}: fell back: {}", report.fallback_reasons.join(" | "));
    } else {
        assert!(
            safety.layer_thickness_within_bounds,
            "{name}: curved file outside the layer window (min {:?} max {:?} of {} samples): {:?}",
            safety.min_physical_layer_mm,
            safety.max_physical_layer_mm,
            safety.layer_thickness_samples,
            safety
                .violations
                .iter()
                .filter(|v| v.kind == "layer_thickness")
                .map(|v| v.detail.clone())
                .collect::<Vec<_>>()
        );
    }
    let scan = scan_thickness(gcode);
    assert!(
        scan.pairs > 500,
        "{name}: only {} measured layer pairs, the scan is not seeing the part",
        scan.pairs
    );
    assert!(
        scan.violations.is_empty(),
        "{name}: {} measured layer pairs outside [{MIN_LAYER}, {MAX_LAYER}] mm (+{MEASURE_TOL}): {:?}",
        scan.violations.len(),
        scan.violations
    );
    assert!(
        scan.min_mm >= MIN_LAYER - MEASURE_TOL,
        "{name}: thinnest deposited layer {:.3} mm, floor is {MIN_LAYER}",
        scan.min_mm
    );
    eprintln!(
        "{name}: {:.3}-{:.3} mm layers over {} measured pairs",
        scan.min_mm, scan.max_mm, scan.pairs
    );
}

#[test]
fn demo_lid_layers_stay_inside_the_slicer_window() {
    if !configured() {
        eprintln!("skipping: engine env not configured");
        return;
    }
    let (gcode, report) = slice(&example("nonplanar-demo.stl"), "demo-lid");
    check("demo lid", &gcode, &report);
}

#[test]
fn dome_layers_stay_inside_the_slicer_window() {
    if !configured() {
        eprintln!("skipping: engine env not configured");
        return;
    }
    let mesh = primitives::sphere(12.0);
    let (gcode, report) = slice(&mesh, "dome");
    check("12 mm dome", &gcode, &report);
}

/// The one fixture whose curved pass survives today's clamp budget, so the
/// G-code scan also runs against a genuinely curved file and not a fallback.
///
/// Its top island is already flat, so the map lifts nothing; the lid and dome
/// above carry the lift and the field-level thickness assertion.
#[test]
fn t_wing_layers_stay_inside_the_slicer_window() {
    if !configured() {
        eprintln!("skipping: engine env not configured");
        return;
    }
    let (gcode, report) = slice(&example("t-wing.stl"), "t-wing");
    check("t-wing", &gcode, &report);
}
