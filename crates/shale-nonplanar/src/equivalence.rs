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

fn normalize_motion(line: &str) -> String {
    let t = line.trim();
    let mut parts: Vec<String> = Vec::new();
    parts.push(if t.starts_with("G0") {
        "G0".to_string()
    } else {
        "G1".to_string()
    });
    for axis in ['X', 'Y', 'Z', 'E', 'F'] {
        if let Some(v) = word_f(t, axis) {
            parts.push(format!("{axis}{v:.3}"));
        }
    }
    parts.join(" ")
}

/// Collect normalized motion lines whose Z is strictly below `band_floor_z`.
pub fn motion_lines_below_band(gcode: &str, band_floor_z: f64) -> Vec<String> {
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

/// Compare planar Cura output to the finished non-planar G-code below the curved band.
pub fn check_planar_equivalence(
    planar_gcode: &str,
    curved_gcode: &str,
    band_floor_z: f64,
) -> PlanarEquivalenceResult {
    let planar = motion_lines_below_band(planar_gcode, band_floor_z);
    let curved = motion_lines_below_band(curved_gcode, band_floor_z);
    let mut mismatched = 0usize;
    let mut first: Option<String> = None;
    let common = planar.len().min(curved.len());
    for i in 0..common {
        if planar[i] != curved[i] {
            mismatched += 1;
            if first.is_none() {
                first = Some(format!("index {i}: planar `{}` vs curved `{}`", planar[i], curved[i]));
            }
        }
    }
    if planar.len() != curved.len() {
        mismatched += planar.len().abs_diff(curved.len());
        first.get_or_insert_with(|| {
            format!(
                "length mismatch: planar {} moves, curved {}",
                planar.len(),
                curved.len()
            )
        });
    }
    PlanarEquivalenceResult {
        ok: mismatched == 0,
        band_floor_z,
        planar_moves_below_band: planar.len(),
        curved_moves_below_band: curved.len(),
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