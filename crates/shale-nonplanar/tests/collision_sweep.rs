//! Synthetic swept-envelope tests: hand-written G-code with a known answer.
//!
//! These pin the four acceptance cases (extruding strike, clear path, planar
//! print) plus mid-segment detection, arc accounting and the planar fast path.

use std::time::Instant;

use shale_nonplanar::collision::{find_collisions, find_collisions_with_stats, CollisionHit};
use shale_nonplanar::envelope::bambu_a1_envelope;

const STEP: f64 = 0.35;
const ORIGIN: [f64; 2] = [0.0, 0.0];
const WIDTH: usize = 600; // 210 mm at 0.35 mm
const HEIGHT: usize = 600;

fn hits(gcode: &str) -> Vec<CollisionHit> {
    find_collisions(gcode, bambu_a1_envelope(), ORIGIN, STEP, WIDTH, HEIGHT)
}

/// A 10 mm pillar at (20, 20) printed as one vertical extrusion.
const PILLAR: &str = "G0 X20 Y20 Z0.2\nG1 X20 Y20 Z10 E1\n";

#[test]
fn extruding_move_beside_a_six_millimetre_wall_is_a_hit() {
    // Wall top 10 mm, extruding pass at Z=4 ends 10 mm away: 6 mm of material
    // above the tip inside the 12 mm body radius collides (allowed: 4.76 mm).
    let gcode = format!(
        "{PILLAR}\
G0 X40 Y20 Z10.5\n\
G0 X40 Y20 Z4\n\
G1 X30 Y20 Z4 E2\n"
    );
    let found = hits(&gcode);
    assert_eq!(found.len(), 1, "{found:#?}");
    let hit = &found[0];
    assert!(hit.extruding, "{hit:#?}");
    assert!((hit.z - 4.0).abs() < 1e-9, "{hit:#?}");
    assert!((hit.material_above_tip_mm - 6.0).abs() < 1e-6, "{hit:#?}");
    assert!(hit.distance_mm <= 12.0, "{hit:#?}");
    assert_eq!(hit.line, 5);
}

#[test]
fn the_same_move_thirty_millimetres_away_is_clear() {
    let gcode = format!(
        "{PILLAR}\
G0 X60 Y20 Z10.5\n\
G0 X60 Y20 Z4\n\
G1 X55 Y20 Z4 E2\n"
    );
    assert!(hits(&gcode).is_empty());
}

#[test]
fn travel_through_printed_material_is_a_travel_hit() {
    let gcode = format!(
        "{PILLAR}\
G0 X20 Y40 Z10.5\n\
G0 X20 Y40 Z4\n\
G0 X20 Y0 Z4\n"
    );
    let found = hits(&gcode);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(!found[0].extruding, "{found:#?}");
    assert!((found[0].z - 4.0).abs() < 1e-9, "{found:#?}");
}

#[test]
fn planar_print_over_itself_never_collides() {
    let gcode = "\
G0 X10 Y10 Z0.2
G1 X40 Y10 Z0.2 E0.2
G1 X40 Y40 Z0.2 E0.6
G1 X10 Y40 Z0.2 E1.0
G1 X10 Y10 Z0.2 E1.4
G1 X40 Y10 Z0.4 E1.8
G1 X40 Y40 Z0.4 E2.2
G1 X10 Y40 Z0.4 E2.6
G1 X10 Y10 Z0.4 E3.0
G1 X40 Y40 Z0.6 E3.4
G1 X10 Y40 Z0.6 E3.8
G1 X10 Y10 Z0.6 E4.2
G1 X40 Y10 Z0.6 E4.6
";
    assert!(hits(gcode).is_empty());
}

#[test]
fn collision_in_the_middle_of_a_segment_is_found() {
    // Endpoints are 20 mm from the pillar (4.7 mm of material clears the 6 mm
    // duct rule there); the middle of the travel passes right over it, where
    // the cone only allows the bead tolerance.
    let gcode = "\
G0 X20 Y20 Z0.2
G1 X20 Y20 Z6.7 E1
G0 X20 Y0 Z6.7
G0 X20 Y0 Z2
G0 X20 Y40 Z2
";
    let found = hits(gcode);
    assert_eq!(found.len(), 1, "{found:#?}");
    let hit = &found[0];
    assert!(!hit.extruding, "{hit:#?}");
    assert!(
        hit.y > 5.0 && hit.y < 35.0,
        "hit must be mid-segment, not at an endpoint: {hit:#?}"
    );
    assert_eq!(hit.line, 5);
}

#[test]
fn arcs_are_counted_and_sampled_by_endpoint() {
    let gcode = "G2 X10 Y10 Z2 I5 J0 E1\n";
    let (found, stats) = find_collisions_with_stats(
        gcode,
        bambu_a1_envelope(),
        ORIGIN,
        STEP,
        WIDTH,
        HEIGHT,
        None,
    );
    assert!(found.is_empty());
    assert_eq!(stats.arc_moves, 1);
    assert_eq!(stats.extruding_moves, 1);
}

#[test]
fn comments_and_non_move_lines_do_not_print_material() {
    // If the commented-out extrusion were parsed the pillar would top out at
    // 30.5 mm and the pass at Z=24 would hit it (6.5 mm > 6 mm duct allowance).
    let gcode = "\
; G1 X20 Y20 Z30.5 E5
M82
G92 E0
G0 X20 Y20 Z0.2
G1 X20 Y20 Z10 E1
G0 X20 Y0 Z24
";
    assert!(hits(gcode).is_empty());
}

#[test]
fn planar_sweep_of_two_hundred_thousand_moves_stays_under_two_seconds() {
    let mut gcode = String::with_capacity(12 * 1024 * 1024);
    gcode.push_str("M82\nG0 X5 Y10 Z0.2\n");
    let mut e = 0.0f64;
    for layer in 0..200 {
        let z = 0.2 + 0.2 * layer as f64;
        for step in 0..1000 {
            let line = step / 50;
            let index = step % 50;
            let y = 10.0 + line as f64;
            let x = if line % 2 == 0 {
                5.0 + 1.4 * index as f64
            } else {
                5.0 + 1.4 * (49 - index) as f64
            };
            e += 0.05;
            gcode.push_str(&format!("G1 X{x:.3} Y{y:.3} Z{z:.3} E{e:.3}\n"));
        }
    }
    let moves = gcode.lines().filter(|line| line.starts_with("G1 ")).count();
    assert_eq!(moves, 200_000);
    let started = Instant::now();
    let found = hits(&gcode);
    let elapsed = started.elapsed();
    eprintln!("swept {moves} planar moves in {elapsed:?}");
    assert!(found.is_empty(), "{} hits", found.len());
    assert!(
        elapsed.as_secs_f64() < 2.0,
        "200k planar moves took {elapsed:?}"
    );
}
