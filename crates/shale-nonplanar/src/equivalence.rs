//! Assert G-code below the non-planar band matches a planar slice.

use crate::config::PlanarEquivalenceResult;

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
    if t.starts_with("G28") || t.starts_with("G90") || t.starts_with("G91") || t.starts_with("G92") {
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

fn normalize_motion(line: &str) -> String {
    let t = line.trim();
    let g0 = t.starts_with("G0");
    let mut parts: Vec<String> = Vec::new();
    parts.push(if g0 { "G0".to_string() } else { "G1".to_string() });
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
        if is_motion_line(line) && position[2] + 1e-6 >= band_floor_z {
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

/// Compare planar Cura output to grafted non-planar output below the curved band.
pub fn check_planar_equivalence(
    planar_gcode: &str,
    curved_gcode: &str,
    band_floor_z: f64,
) -> PlanarEquivalenceResult {
    let planar_split =
        first_band_motion_line(planar_gcode, band_floor_z).unwrap_or_else(|| planar_gcode.lines().count());
    let curved_split =
        first_band_motion_line(curved_gcode, band_floor_z).unwrap_or_else(|| curved_gcode.lines().count());
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
    }
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