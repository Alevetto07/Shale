//! The A1's own start and end sequence.
//!
//! `start.gcode` and `end.gcode` are the two templates Bambu Studio keeps
//! beside `Bambu Lab A1 0.4 nozzle.json` — `… template machine_start_gcode.json`
//! and its end counterpart — joined into plain text. They are here as data so
//! this test runs on a machine with no Bambu Studio installed, and so a change
//! in the fill rules cannot quietly pass by reading a newer profile.

use std::collections::BTreeMap;

use shale_core::bambu::MachineGcode;
use shale_core::job::{render_sequence, PlacedModel};
use shale_core::mesh::{Transform, TriangleMesh};
use shale_core::primitives;
use shale_core::resolver::ProfileSummary;
use shale_core::resolver::ResolvedSettings;

const START: &str = include_str!("fixtures/bambu-a1/start.txt");
const END: &str = include_str!("fixtures/bambu-a1/end.txt");

/// What the resolver hands a slice of the A1 at 0.4 mm with PLA: Cura's names,
/// the values Bambu's template asks for under other names.
fn resolved() -> ResolvedSettings {
    let mut global = BTreeMap::new();
    for (key, value) in [
        ("material_print_temperature", "210"),
        ("material_print_temperature_layer_0", "215"),
        ("material_bed_temperature", "60"),
        ("material_bed_temperature_layer_0", "60"),
        ("machine_nozzle_size", "0.4"),
        ("magic_spiralize", "false"),
        ("print_sequence", "all_at_once"),
        ("filament_max_volumetric_speed", "21"),
        ("layer_height", "0.2"),
        ("wall_line_width", "0.42"),
    ] {
        global.insert(key.to_string(), value.to_string());
    }
    ResolvedSettings {
        printer_id: "bambu_lab_a1".into(),
        printer_name: "Bambu Lab A1".into(),
        variant_name: "0.4 mm".into(),
        quality_type: "0.20mm Standard @BBL A1".into(),
        material_id: "bambu_bambu_pla_basic".into(),
        extruders: vec![global.clone()],
        global,
        summary: summary(),
        warnings: Vec::new(),
        warning_count: 0,
    }
}

fn summary() -> ProfileSummary {
    ProfileSummary {
        printer_name: "Bambu Lab A1".into(),
        machine_width: 256.0,
        machine_depth: 256.0,
        machine_height: 256.0,
        machine_center_is_zero: false,
        layer_height: 0.2,
        line_width: 0.42,
        infill_sparse_density: 15.0,
        infill_line_distance: 2.8,
        support_enable: false,
        nozzle_temperature: 210.0,
        bed_temperature: 60.0,
        nozzle_size: 0.4,
        adhesion_type: "skirt".into(),
        speed_print: 60.0,
        material_diameter: 1.75,
    }
}

fn cube() -> PlacedModel {
    PlacedModel {
        name: "cube.stl".into(),
        mesh: primitives::box_solid(10.0, 10.0, 10.0),
        transform: Transform {
            translation: [100.0, 100.0, 0.0],
            ..Transform::default()
        },
        settings: Vec::new(),
        slot: None,
    }
}

fn sequence() -> (String, String) {
    let gcode = MachineGcode {
        start: START.to_string(),
        end: END.to_string(),
    };
    render_sequence(&gcode, "PLA", &resolved(), &[cube()]).expect("the A1 sequence fills")
}

#[test]
fn the_a1_starts_with_bambu_and_not_with_marlin() {
    let (start, _) = sequence();
    let head: Vec<&str> = start.lines().take(2).collect();
    assert_eq!(
        head.first().copied(),
        Some(";===== machine: A1 ========================="),
        "head: {head:?}"
    );
    // The three things Cura's generic start never does: hand over to the AMS,
    // home Z against the bed, and let the machine calibrate its own extrusion.
    assert!(start.contains("M620 S0A"), "no AMS handover");
    assert!(start.contains("G28 Z P0 T140"), "no bed-levelling home");
    assert!(start.contains("M983 F"), "no dynamic extrusion calibration");
    assert!(start.contains("wipe and shake"), "no nozzle wipe");
    assert!(
        !start.contains('{') && !start.contains('['),
        "a placeholder was left in the start sequence"
    );
}

#[test]
fn the_a1_ends_the_way_bambu_ends_it() {
    let (_, end) = sequence();
    let tail: Vec<&str> = end.trim_end().lines().rev().take(3).collect();
    assert_eq!(
        tail,
        vec![
            "M18 X Y Z",
            "M400",
            ";M17 X0.8 Y0.8 Z0.5 ; lower motor current to 45% power",
        ],
        "tail: {tail:?}"
    );
    assert!(end.contains("M400 ; wait for buffer to clear"));
    assert!(
        !end.contains('{') && !end.contains('['),
        "a placeholder was left in the end sequence"
    );
}

#[test]
fn the_numbers_in_the_sequence_are_this_slice_numbers() {
    let (start, _) = sequence();
    // Layer 0 at 215, the rest at 210: the first-layer name and the other one
    // are different settings and the template uses both.
    assert!(start.contains("M104 S215"), "no first-layer temperature");
    assert!(start.contains("M109 S215"), "the first-layer wait is missing");
    // `M109 S{nozzle_temperature[initial_extruder]}` is the print temperature,
    // and it is the other name: 210, not the 215 the first layer heats to.
    assert!(start.contains("M109 S210"), "no print temperature");
    assert!(start.contains("M140 S60"), "no bed temperature");
    // Nozzle 0.4, and the bed scan is the textured-plate one the A1 ships with.
    assert!(start.contains("H0.4"), "the nozzle diameter is not in the sequence");
    assert!(
        start.contains("Textured PEI Plate"),
        "the bed type did not reach the template"
    );
    // The filament type goes to the machine, which uses it for its own limits.
    assert!(start.contains("set_filament_type:PLA"), "no filament type");
}

#[test]
fn a_placeholder_with_no_value_stops_the_slice() {
    let gcode = MachineGcode {
        start: "M104 S{nozzle_temperature_at_noon}".into(),
        end: String::new(),
    };
    let error = render_sequence(&gcode, "PLA", &resolved(), &[cube()])
        .expect_err("a name with no value must refuse");
    assert!(
        error.to_string().contains("nozzle_temperature_at_noon"),
        "{error}"
    );
}
