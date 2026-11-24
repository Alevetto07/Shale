// SPDX-License-Identifier: AGPL-3.0-or-later
//! A curved slice may never print *into* material it already laid.
//!
//! The deformation maps every G-code point back to the physical Z it was meant
//! for, and the slicer keeps producing layers bottom-up in deformed space. If a
//! column's mapping is stronger than its neighbour's, the mapped layer order can
//! invert: a later extruding move lands below plastic that is already there at
//! the same XY. That is a gouge — the nozzle ploughs through the top it just
//! printed — and it is a property of the whole file, not of the field, so it has
//! to be measured on the G-code the printer actually receives.
//!
//! Metric: walk every extruding move, sample it at most 0.25 mm apart into a
//! 0.4 mm XY lattice and remember the highest Z printed in each cell. A sample
//! that sits more than [`INVERSION_DROP_MM`] below that height is an inversion.
//! The planar slice of the same part runs through the same scan and must report
//! zero, which calibrates the lattice: no retraction, seam or ironing pattern of
//! the reference slice is allowed to move the baseline.
//!
//! Why the tolerance is one head-clearance unit and not a hair: the band floor is
//! anchored per island at `h_top_smooth - band`, which lands between two planar
//! layers, so the first band layer can print a fraction of a layer under the last
//! planar one at the footprint seam. Measured on the t-wing and the 24 mm ball
//! (M1-B4) that overlap is 0.057-0.061 mm — a quarter layer, inside the 0.1 mm the
//! swept toolhead check itself allows, and an overlap rather than a gap, which is
//! the safe side. Counting it would make the scan assert something the product
//! deliberately permits; the rim collapse this test exists for dropped 0.69 mm, so
//! 0.1 mm still catches it with room to spare.
//!
//! Fixtures: `examples/t-wing.stl` (flat 40x10x4 deck on a 10x10x20 pillar; the
//! deck edge is the only part of its top that is not horizontal) and the 12 mm
//! dome (`primitives::sphere(12.0)`), the curved top the curved band exists for.

use std::collections::HashMap;
use std::path::PathBuf;

use shale_core::engine::CliEngine;
use shale_core::job::{self, PlacedModel};
use shale_core::mesh::{self, TriangleMesh};
use shale_core::primitives;
use shale_core::resolver::UserSettings;
use shale_core::{Catalog, NonplanarMode, NonplanarOptions, ResourcePaths};

const PRINTER: &str = "bambu_lab_a1";
const VARIANT: &str = "0.4 mm";
const QUALITY: &str = "0_20mm_standard_bbl_a1";
const MATERIAL: &str = "bambu_lab_pla_basic";

/// XY lattice the printed-height map is kept on, in millimetres.
const CELL_MM: f32 = 0.4;
/// How far a move may dip below already-printed plastic before it counts.
///
/// One head-clearance unit (the sweep's own 0.1 mm), not a hair: the band floor is
/// anchored between two planar layers, so the first band layer legitimately
/// overlaps the last planar one by up to a quarter of a layer at the seam
/// (0.061 mm measured on the t-wing). See the module docs.
const INVERSION_DROP_MM: f32 = 0.1;
/// Longest sample spacing along an extruding move, in millimetres.
const SAMPLE_MM: f32 = 0.25;

fn configured() -> bool {
    std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some()
        && std::env::var_os("SHALE_ORCA_DIR").is_some()
}

fn fixture(name: &str) -> TriangleMesh {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    mesh::load_stl(&bytes).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()))
}

fn slice(mesh: &TriangleMesh, mode: NonplanarMode, tag: &str) -> String {
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available(), "CuraEngine not available");
    let mut settings = UserSettings {
        layer_height: Some(0.2),
        ..UserSettings::default()
    };
    settings.nonplanar = NonplanarOptions {
        mode,
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
        .join(format!("z-inversion-{tag}-{}.gcode", std::process::id()));
    job::slice(
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
    std::fs::read_to_string(path).expect("gcode")
}

/// `(inversions, first offender)` for one printed file.
///
/// Travel moves are skipped: a travel through printed material is a collision,
/// which the safety pass already reports; this scan is about the toolpath
/// descending into the part while extruding.
fn z_inversions(gcode: &str) -> (usize, Option<String>) {
    let toolpath = shale_core::gcode::parse_toolpaths(gcode);
    let mut printed: HashMap<(i32, i32), f32> = HashMap::new();
    let mut hits = 0usize;
    let mut first = None;
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
                        if first.is_none() {
                            first = Some(format!(
                                "X{x:.3} Y{y:.3} Z{z:.3} below printed {:.3} at the same XY",
                                *seen
                            ));
                        }
                    } else if z > *seen {
                        *seen = z;
                    }
                }
            }
        }
    }
    (hits, first)
}

fn assert_clean(name: &str, gcode: &str) {
    let (hits, first) = z_inversions(gcode);
    assert_eq!(
        hits, 0,
        "{name}: {hits} extruding move(s) below already-printed material (first: {first:?})"
    );
}

#[test]
fn planar_reference_has_no_z_inversions() {
    if !configured() {
        eprintln!("skipping: SHALE_CURAENGINE / resource paths not set");
        return;
    }
    let dome = primitives::sphere(12.0);
    let cases: [(&str, TriangleMesh); 2] = [("t-wing", fixture("t-wing.stl")), ("dome", dome)];
    for (name, mesh) in cases {
        let gcode = slice(&mesh, NonplanarMode::Off, &format!("planar-{name}"));
        assert_clean(&format!("{name} planar"), &gcode);
    }
}

#[test]
fn curved_output_never_prints_below_itself() {
    if !configured() {
        eprintln!("skipping: SHALE_CURAENGINE / resource paths not set");
        return;
    }
    let dome = primitives::sphere(12.0);
    let cases: [(&str, TriangleMesh); 2] = [("t-wing", fixture("t-wing.stl")), ("dome", dome)];
    for (name, mesh) in cases {
        for mode in [NonplanarMode::Auto, NonplanarMode::CurvedTops] {
            let tag = format!("{name}-{}", mode.name());
            let gcode = slice(&mesh, mode, &tag);
            assert_clean(&tag, &gcode);
        }
    }
}
