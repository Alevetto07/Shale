//! Regression for the chevron false positive: a 10 deg serpentine band whose
//! neighbouring beads sit 0.42 mm apart and a fraction of a millimetre higher
//! than the nozzle tip.
//!
//! Real CuraEngine output for the chevron fixture failed with
//! "passes 0.11 mm of printed material 0.28 mm from the nozzle axis". The beads
//! are neighbours of the same curved band, not head strikes: the printed map
//! snapped bead centrelines to the deform field's 0.35 mm cell centres, so a
//! bead 0.42 mm away was measured at 0.28 mm, inside the A1's 0.3 mm tip land,
//! where the envelope allows no material above the tip at all.
//!
//! The map now has its own 0.1 mm cells and measures to the stored sample, so
//! this band is clear at every angle while a move that really plunges 0.5 mm
//! into the previous line is still a hit.

use shale_nonplanar::collision::{find_collisions, CollisionHit, PRINTED_MAP_CELL_MM};
use shale_nonplanar::envelope::bambu_a1_envelope;

/// The deform field resolution used by the fixtures that failed. The collision
/// map must not inherit it.
const FIELD_STEP: f64 = 0.35;
const ORIGIN: [f64; 2] = [0.0, 0.0];
const WIDTH: usize = 600; // 210 mm at 0.35 mm
const HEIGHT: usize = 600;

/// Bead-to-bead spacing of the failing fixture's 10 deg band.
const LINE_SPACING_MM: f64 = 0.42;
/// Height gained per line: 0.11 mm, the exact height the failing fixtures
/// reported above the tip.
const RISE_PER_LINE_MM: f64 = 0.11;
const LINES: usize = 12;
const LINE_LENGTH_MM: f64 = 20.0;
const BASE: [f64; 2] = [60.0, 60.0];
const BASE_Z: f64 = 3.0;

/// A boustrophedon band of `LINES` parallel lines, rotated by `angle_deg` so the
/// beads land at irrational offsets in any map lattice.
///
/// Each line descends by [`RISE_PER_LINE_MM`] from the level the previous line
/// ended at, the way CuraEngine emits a curved surface: the travel keeps the old
/// Z and the descent happens inside the following extruding move. The nozzle is
/// therefore always catching up with the band one bead behind it, which is the
/// geometry that produced the false positives.
///
/// `drop_last_line_mm` lowers the final extruding move by that much, so the
/// nozzle runs 0.5 mm below the band it just laid.
fn band(angle_deg: f64, drop_last_line_mm: f64) -> String {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let direction = [cos, sin];
    let normal = [-sin, cos];
    let level = |line: usize| BASE_Z - RISE_PER_LINE_MM * line as f64;
    let mut gcode = String::from("M82\n");
    let mut e = 0.0;
    for line in 0..LINES {
        let along = [
            BASE[0] + normal[0] * line as f64 * LINE_SPACING_MM,
            BASE[1] + normal[1] * line as f64 * LINE_SPACING_MM,
        ];
        let far = [
            along[0] + direction[0] * LINE_LENGTH_MM,
            along[1] + direction[1] * LINE_LENGTH_MM,
        ];
        // Serpentine: odd lines run backwards, so every hop is a 0.42 mm lateral
        // move at the level the previous line finished on.
        let (from, to, from_z, to_z) = if line % 2 == 0 {
            (along, far, level(line), level(line + 1))
        } else {
            (far, along, level(line), level(line + 1))
        };
        // A dropped line sits 0.5 mm below the band for its whole length: the
        // travel arrives on the band and the nozzle plunges straight down at the
        // start of the line, the way a real Z offset would be applied.
        let (extrude_from_z, extrude_to_z) = if line + 1 == LINES {
            (from_z - drop_last_line_mm, to_z - drop_last_line_mm)
        } else {
            (from_z, to_z)
        };
        e += 1.0;
        gcode.push_str(&format!(
            "G0 X{:.4} Y{:.4} Z{:.4}\n",
            from[0], from[1], from_z
        ));
        if (extrude_from_z - from_z).abs() > 1e-9 {
            gcode.push_str(&format!("G0 Z{:.4}\n", extrude_from_z));
        }
        gcode.push_str(&format!(
            "G1 X{:.4} Y{:.4} Z{:.4} E{:.5}\n",
            to[0], to[1], extrude_to_z, e
        ));
    }
    gcode
}

fn hits(gcode: &str) -> Vec<CollisionHit> {
    find_collisions(
        gcode,
        bambu_a1_envelope(),
        ORIGIN,
        FIELD_STEP,
        WIDTH,
        HEIGHT,
    )
}

#[test]
fn map_is_finer_than_the_field_grid_the_fixtures_use() {
    // Compile-time: the map must be finer than the 0.35 mm deform grid that
    // produced the chevron false positive, and much finer than a bead.
    const {
        assert!(PRINTED_MAP_CELL_MM <= 0.1);
        assert!(PRINTED_MAP_CELL_MM < LINE_SPACING_MM / 4.0);
    }
}

#[test]
fn ten_degree_band_at_real_spacing_is_clear() {
    for angle in [0.0, 10.0, 37.3, 61.4, 89.0] {
        let found = hits(&band(angle, 0.0));
        assert!(
            found.is_empty(),
            "{angle} deg band printed {} hit(s): {found:#?}",
            found.len()
        );
    }
}

#[test]
fn a_move_dropped_half_a_millimetre_into_the_previous_line_is_a_hit() {
    let found = hits(&band(10.0, 0.5));
    // The plunge itself is a strike, and so is the whole dropped line that
    // follows it; the extruding hit is the one that has to be unambiguous.
    let extruding: Vec<&CollisionHit> = found.iter().filter(|h| h.extruding).collect();
    assert_eq!(extruding.len(), 1, "{found:#?}");
    let hit = extruding[0];
    assert!(
        (hit.distance_mm - LINE_SPACING_MM).abs() < 0.02,
        "distance must be measured to the neighbouring bead: {hit:#?}"
    );
    assert!(
        (hit.material_above_tip_mm - 0.5).abs() < 0.02,
        "material height must be the 0.5 mm the line dropped by: {hit:#?}"
    );
}

#[test]
fn the_clear_band_stays_clear_at_every_field_resolution() {
    // The map sizes itself from the material, so an absurd field resolution
    // changes nothing: 0.35 mm (the fixtures), 2 mm and 5 mm all agree.
    let gcode = band(10.0, 0.0);
    for step in [0.35, 2.0, 5.0] {
        let found = find_collisions(&gcode, bambu_a1_envelope(), ORIGIN, step, 600, 600);
        assert!(found.is_empty(), "field step {step}: {found:#?}");
    }
}
