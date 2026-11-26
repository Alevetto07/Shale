//! Assert G-code below the non-planar band matches a planar slice.

use crate::config::{CurvedSuffixBelowFloorMoves, PlanarEquivalenceResult};
use crate::deform::DeformField;

/// Matches inverse band round-trip tests (`box_top_band_round_trip_under_0_02mm`).
pub const SUFFIX_SHELL_EXTRUSION_TOLERANCE_MM: f64 = 0.02;

/// Ignore comments, headers, and post-pass annotations when comparing moves.
fn is_ignorable_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    if !t.starts_with('G') {
        return true;
    }
    if t.starts_with(";SHALE_") || t.contains("SHALE_NONPLANAR") {
        return true;
    }
    if t.starts_with("G28") || t.starts_with("G90") || t.starts_with("G91") || t.starts_with("G92")
    {
        return false;
    }
    t.starts_with('G') && (t.contains('X') || t.contains('Y') || t.contains('Z'))
}

fn word_f(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn is_motion_line(line: &str) -> bool {
    line.starts_with('G') && (line.contains('X') || line.contains('Y'))
}

fn is_extrusion_motion(line: &str) -> bool {
    line.starts_with("G1")
        && line.contains('E')
        && is_motion_line(line)
        && word_f(line, 'E').is_some_and(|e| e > 1e-9)
}

fn normalize_motion(line: &str) -> String {
    let t = line.trim();
    let g0 = t.starts_with("G0");
    let mut parts: Vec<String> = Vec::new();
    parts.push(if g0 {
        "G0".to_string()
    } else {
        "G1".to_string()
    });
    // Travel moves may carry stale E on Marlin; compare XY(ZF) only.
    let axes: &[char] = if g0 {
        &['X', 'Y', 'Z', 'F']
    } else {
        &['X', 'Y', 'Z', 'E', 'F']
    };
    for axis in axes {
        if let Some(v) = word_f(t, *axis) {
            if *axis == 'F' {
                parts.push(format!("F{v:.0}"));
            } else {
                parts.push(format!("{axis}{v:.3}"));
            }
        }
    }
    parts.join(" ")
}

/// Collect normalized motion lines whose Z is strictly below `band_floor_z`.
pub fn motion_lines_below_band(gcode: &str, band_floor_z: f64) -> Vec<String> {
    motion_lines_shell_region(gcode, band_floor_z, None)
}

/// Motion lines from the planar shell prefix only (before the first at-or-above-band XY move).
fn motion_lines_shell_region(
    gcode: &str,
    band_floor_z: f64,
    line_limit: Option<usize>,
) -> Vec<String> {
    let split = line_limit.unwrap_or_else(|| {
        first_band_motion_line(gcode, band_floor_z).unwrap_or_else(|| gcode.lines().count())
    });
    let mut position = [0.0f64; 3];
    let mut out = Vec::new();
    for (idx, raw) in gcode.lines().enumerate() {
        if idx >= split {
            break;
        }
        let line = raw.trim();
        if let Some(x) = word_f(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = word_f(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = word_f(line, 'Z') {
            position[2] = z;
        }
        if !is_ignorable_line(line) {
            continue;
        }
        if !(line.starts_with('G') && (line.contains('X') || line.contains('Y'))) {
            continue;
        }
        if position[2] + 1e-6 < band_floor_z {
            out.push(normalize_motion(line));
        }
    }
    out
}

/// Raw motion lines (comments preserved) below `band_floor_z`, in file order.
pub fn motion_raw_below_band(gcode: &str, band_floor_z: f64) -> Vec<String> {
    let mut position = [0.0f64; 3];
    let mut out = Vec::new();
    for raw in gcode.lines() {
        let line = raw.trim();
        if let Some(x) = word_f(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = word_f(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = word_f(line, 'Z') {
            position[2] = z;
        }
        if !is_motion_line(line) {
            continue;
        }
        if position[2] + 1e-6 < band_floor_z {
            out.push(raw.to_string());
        }
    }
    out
}

/// Last line index (exclusive end) still part of the curved-top print body.
fn curved_suffix_end_line(gcode: &str, band_floor_z: f64, start_line: usize) -> usize {
    let mut position = [0.0f64; 3];
    let mut last_at_band = start_line;
    for (idx, raw) in gcode.lines().enumerate() {
        if idx < start_line {
            let line = raw.trim();
            if let Some(x) = word_f(line, 'X') {
                position[0] = x;
            }
            if let Some(y) = word_f(line, 'Y') {
                position[1] = y;
            }
            if let Some(z) = word_f(line, 'Z') {
                position[2] = z;
            }
            continue;
        }
        let line = raw.trim();
        if let Some(x) = word_f(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = word_f(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = word_f(line, 'Z') {
            position[2] = z;
        }
        if is_extrusion_motion(line) && position[2] + 1e-6 >= band_floor_z {
            last_at_band = idx + 1;
        }
    }
    last_at_band + 1
}

fn first_band_motion_line(gcode: &str, band_floor_z: f64) -> Option<usize> {
    let mut position = [0.0f64; 3];
    for (idx, raw) in gcode.lines().enumerate() {
        let line = raw.trim();
        if let Some(x) = word_f(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = word_f(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = word_f(line, 'Z') {
            position[2] = z;
        }
        if is_extrusion_motion(line) && position[2] + 1e-6 >= band_floor_z {
            return Some(idx);
        }
    }
    None
}

/// Replace below-band motion in `mapped` with the planar baseline slice so shell G-code matches.
pub fn graft_planar_shell_below_band(
    planar_baseline_gcode: &str,
    mapped: &str,
    band_floor_z: f64,
) -> String {
    let Some(split_base) = first_band_motion_line(planar_baseline_gcode, band_floor_z) else {
        return mapped.to_string();
    };
    let Some(split_mapped) = first_band_motion_line(mapped, band_floor_z) else {
        return mapped.to_string();
    };
    let base_lines: Vec<&str> = planar_baseline_gcode.lines().collect();
    let mapped_lines: Vec<&str> = mapped.lines().collect();
    let mut out = String::with_capacity(mapped.len() + planar_baseline_gcode.len() / 2);
    for raw in &base_lines[..split_base] {
        out.push_str(raw);
        out.push('\n');
    }
    for raw in &mapped_lines[split_mapped..] {
        out.push_str(raw);
        out.push('\n');
    }
    out
}

/// Classify XY moves in the curved suffix that still sit below `global_band_floor_z()`.
pub fn audit_curved_suffix_below_global_floor(
    grafted_gcode: &str,
    planar_baseline_gcode: &str,
    field: &DeformField,
) -> CurvedSuffixBelowFloorMoves {
    let band_floor = field.global_band_floor_z();
    let split = first_band_motion_line(planar_baseline_gcode, band_floor)
        .unwrap_or_else(|| grafted_gcode.lines().count());
    let end_line = curved_suffix_end_line(grafted_gcode, band_floor, split);
    let mut position = [0.0f64; 3];
    let mut e = None::<f64>;
    let mut audit = CurvedSuffixBelowFloorMoves {
        band_floor_z: band_floor,
        shell_extrusion_tolerance_mm: SUFFIX_SHELL_EXTRUSION_TOLERANCE_MM,
        ok: true,
        ..Default::default()
    };

    for (idx, raw) in grafted_gcode.lines().enumerate() {
        if idx >= end_line {
            break;
        }
        if idx < split {
            let line = raw.trim();
            if let Some(x) = word_f(line, 'X') {
                position[0] = x;
            }
            if let Some(y) = word_f(line, 'Y') {
                position[1] = y;
            }
            if let Some(z) = word_f(line, 'Z') {
                position[2] = z;
            }
            if let Some(ev) = word_f(line, 'E') {
                e = Some(ev);
            }
            continue;
        }

        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(x) = word_f(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = word_f(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = word_f(line, 'Z') {
            position[2] = z;
        }
        let prev_e = e;
        let ev = word_f(line, 'E');
        if let Some(v) = ev {
            e = Some(v);
        }

        if position[2] + 1e-6 >= band_floor {
            continue;
        }

        if !line.starts_with('G') {
            audit.mcode_or_other += 1;
            continue;
        }
        if line.starts_with("G28")
            || line.starts_with("G90")
            || line.starts_with("G91")
            || line.starts_with("G92")
        {
            audit.mcode_or_other += 1;
            continue;
        }

        let has_xy = line.contains('X') || line.contains('Y');
        let has_z = line.contains('Z');
        if has_z && !has_xy {
            audit.z_hop += 1;
            continue;
        }
        if !has_xy {
            audit.mcode_or_other += 1;
            continue;
        }

        let de = match (ev, prev_e) {
            (Some(cur), Some(prev)) => cur - prev,
            (Some(cur), None) => cur,
            _ => 0.0,
        };

        if de < -1e-6 {
            audit.wipe_retract += 1;
            continue;
        }
        if !(line.starts_with("G1") && de > 1e-6) {
            audit.travel += 1;
            continue;
        }

        if position[2] + SUFFIX_SHELL_EXTRUSION_TOLERANCE_MM + 1e-6 >= band_floor {
            audit.band_edge_extrusion += 1;
            continue;
        }
        let z_band = field.grid.sample_band_start(position[0], position[1]);
        let shell_ceiling = z_band
            .map(|z| z + SUFFIX_SHELL_EXTRUSION_TOLERANCE_MM)
            .unwrap_or(band_floor - SUFFIX_SHELL_EXTRUSION_TOLERANCE_MM);
        if position[2] <= shell_ceiling + 1e-6 {
            audit.shell_extrusion_violations += 1;
            audit.ok = false;
            audit.first_shell_violation.get_or_insert_with(|| {
                format!(
                    "line {}: G1 extrusion at Z={:.3} ≤ z_band({:.3},{:.3})+{:.3} mm",
                    idx + 1,
                    position[2],
                    position[0],
                    position[1],
                    SUFFIX_SHELL_EXTRUSION_TOLERANCE_MM
                )
            });
        } else {
            audit.band_edge_extrusion += 1;
        }
    }

    audit
}

/// Compare planar Cura output to grafted non-planar output below the curved band.
pub fn check_planar_equivalence(
    planar_gcode: &str,
    curved_gcode: &str,
    band_floor_z: f64,
) -> PlanarEquivalenceResult {
    let planar_split = first_band_motion_line(planar_gcode, band_floor_z)
        .unwrap_or_else(|| planar_gcode.lines().count());
    let curved_split = first_band_motion_line(curved_gcode, band_floor_z)
        .unwrap_or_else(|| curved_gcode.lines().count());
    let planar_moves = motion_lines_shell_region(planar_gcode, band_floor_z, Some(planar_split));
    let curved_moves = motion_lines_shell_region(curved_gcode, band_floor_z, Some(curved_split));
    let mut mismatched = 0usize;
    let mut first: Option<String> = None;
    for (index, (base, curved)) in planar_moves.iter().zip(curved_moves.iter()).enumerate() {
        if base != curved {
            mismatched += 1;
            if first.is_none() {
                first = Some(format!(
                    "motion below band #{index}: `{curved}` vs expected `{base}`"
                ));
            }
        }
    }
    mismatched += planar_moves.len().abs_diff(curved_moves.len());
    if planar_moves.len() != curved_moves.len() && first.is_none() {
        first = Some(format!(
            "motion count below band: {} planar vs {} curved",
            planar_moves.len(),
            curved_moves.len()
        ));
    }
    PlanarEquivalenceResult {
        ok: mismatched == 0,
        band_floor_z,
        planar_moves_below_band: planar_moves.len(),
        curved_moves_below_band: curved_moves.len(),
        mismatched_moves: mismatched,
        first_mismatch: first,
        curved_suffix_below_floor: None,
    }
}

/// Grafted-path checks: shell prefix vs planar, and no planar-shell extrusion in curved suffix.
pub fn check_grafted_planar_equivalence(
    planar_gcode: &str,
    grafted_gcode: &str,
    field: &DeformField,
) -> PlanarEquivalenceResult {
    let band_floor = field.global_band_floor_z();
    let mut result = check_planar_equivalence(planar_gcode, grafted_gcode, band_floor);
    let suffix = audit_curved_suffix_below_global_floor(grafted_gcode, planar_gcode, field);
    result.ok = result.ok && suffix.ok;
    result.curved_suffix_below_floor = Some(suffix);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_below_band_matches() {
        let src = "G1 X0 Y0 Z1 E0\nG1 X10 Y10 Z5 E1\nG1 X20 Y20 Z25 E2\n";
        let band = 20.0;
        let r = check_planar_equivalence(src, src, band);
        assert!(r.ok);
        assert_eq!(r.planar_moves_below_band, 2);
    }

    #[test]
    fn changed_move_above_band_ignored() {
        let planar = "G1 X0 Y0 Z5 E0\nG1 X10 Y10 Z25 E1\n";
        let curved = "G1 X0 Y0 Z5 E0\nG1 X10 Y10 Z26 E1\n";
        let r = check_planar_equivalence(planar, curved, 20.0);
        assert!(r.ok);
    }

    #[test]
    fn graft_restores_planar_shell() {
        let baseline = "G1 X0 Y0 Z1 E0.1\nG1 X1 Y0 Z1 E0.2\nG1 X1 Y1 Z9 E1\n";
        let mapped = ";hdr\nG1 X0 Y0 Z1 E0.5\nG0 X0.5 Y0.5 Z1\nG1 X1 Y1 Z9 E1\n";
        let out = graft_planar_shell_below_band(baseline, mapped, 8.0);
        let eq = check_planar_equivalence(baseline, &out, 8.0);
        assert!(eq.ok, "{eq:?}");
        assert!(out.starts_with("G1 X0 Y0 Z1 E0.1"));
    }

    #[test]
    fn changed_move_below_band_fails() {
        let planar = "G1 X0 Y0 Z5 E0\n";
        let curved = "G1 X0 Y0 Z5.1 E0\n";
        let r = check_planar_equivalence(planar, curved, 20.0);
        assert!(!r.ok);
        assert_eq!(r.mismatched_moves, 1);
    }

    #[test]
    fn suffix_flags_planar_shell_extrusion_after_band_starts() {
        use crate::deform::build_deform_field;
        use crate::height_field::build_height_field;
        use crate::test_meshes::box_mesh;

        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let band = field.global_band_floor_z();
        let gcode = format!(
            "G1 X5 Y5 Z{} E0\nG1 X5 Y5 Z{} E1\nG1 X5 Y5 Z{:.3} E2\n",
            band - 2.0,
            band + 0.5,
            band - 1.0
        );
        let audit = audit_curved_suffix_below_global_floor(&gcode, &gcode, &field);
        assert!(!audit.ok, "{audit:?}");
        assert_eq!(audit.shell_extrusion_violations, 1);
    }

    /// The global floor is the lowest transition start on the plate, not the
    /// tallest column's band. When it was computed from the tallest top, every
    /// move under a short island sat "below the floor" and the inverse map
    /// skipped it (a 0.37 mm error on the bridge fixture). The transition now
    /// starts below the band by the column's own ramp, so the floor sits at or
    /// under the lowest band and must not climb up to the tallest one.
    #[test]
    fn global_floor_is_the_lowest_column_band() {
        use crate::deform::build_deform_field;
        use crate::height_field::build_height_field;
        use crate::test_meshes::sphere_mesh;

        let mut mesh = sphere_mesh(12.0);
        for v in &mut mesh.vertices {
            v[0] += 128.0;
            v[1] += 128.0;
        }
        let grid = build_height_field(&mesh, 0.35, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let floor = field.global_band_floor_z();
        let mut lowest = f64::INFINITY;
        let mut highest = f64::NEG_INFINITY;
        let mut material = 0usize;
        for (i, top) in field.grid.z_top_smooth.iter().enumerate() {
            if top.is_none() {
                continue;
            }
            let band = field.grid.z_band_start[i].expect("material columns carry a band");
            lowest = lowest.min(band);
            highest = highest.max(band);
            material += 1;
        }
        assert!(material > 100, "fixture must have material columns");
        assert!(
            floor <= lowest + 1e-9,
            "floor {floor:.3} must sit at or below the lowest band {lowest:.3}, not the highest {highest:.3}"
        );
        assert!(
            highest - floor > 1.0,
            "floor {floor:.3} collapsed towards the tallest band {highest:.3}"
        );
        // A move inside the lowest column's band needs the map; the tallest column on
        // the plate must not exempt it.
        let (x, y) = (126.2, 124.3);
        let z_band = field.grid.sample_band_start(x, y).expect("sphere column");
        let z_top = field.grid.sample_top_smooth(x, y).expect("sphere top");
        let z = z_band + (z_top - z_band) * 0.5;
        assert!(
            field.gcode_needs_inverse(x, y, field.forward_z(x, y, z)),
            "an in-band move must be mapped (band {z_band:.3}, top {z_top:.3})"
        );
    }

    #[test]
    fn inverse_preserves_low_shell_moves() {
        use crate::deform::build_deform_field;
        use crate::height_field::build_height_field;
        use crate::inverse::map_gcode_inverse;
        use crate::test_meshes::box_mesh;

        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let planar = "G1 X5 Y5 Z2 E0\nG1 X15 Y5 Z2.2 E1\n";
        let mapped = map_gcode_inverse(&field, planar, 0.2);
        let floor = field.global_band_floor_z();
        let r = check_planar_equivalence(planar, &mapped, floor);
        assert!(
            r.ok,
            "{r:?}\nplanar={:?}\ncurved={:?}",
            motion_lines_below_band(planar, floor),
            motion_lines_below_band(&mapped, floor)
        );
    }
}
