//! Slope verification for the curved band.
//!
//! This module only *measures*. Slope limiting happens before slicing, in
//! [`crate::slope_limit`], so that the mesh CuraEngine slices and the field the
//! inverse map uses are the same geometry. A post-slice pass that edited the
//! field could never be the inverse of what was sliced, which is why the old
//! damp-and-remap loop never converged and fell back to silently clamping Z.

use crate::deform::DeformField;

/// Ignore sub-millimetre horizontal hops when chaining extrusions (layer Z-steps at fixed XY).
const VERTICAL_STEP_HORIZ_MM: f64 = 0.05;
/// Reporting / validation slack above `max_angle_deg` (grid quantisation).
pub const SLOPE_CAP_TOLERANCE_DEG: f64 = 0.5;

fn word_f(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// A point is in the curved band when it sits above the column's band start.
pub fn point_in_curved_band(field: &DeformField, p: [f64; 3]) -> bool {
    p[2] + 1e-6 >= field.global_band_floor_z()
        && field
            .grid
            .sample_band_start(p[0], p[1])
            .is_some_and(|z_band| p[2] > z_band + 1e-6)
}

fn segment_in_curved_band(field: &DeformField, a: [f64; 3], b: [f64; 3]) -> bool {
    point_in_curved_band(field, a) && point_in_curved_band(field, b)
}

fn slope_deg(a: [f64; 3], b: [f64; 3]) -> Option<f64> {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let dz = b[2] - a[2];
    let horiz = (dx * dx + dy * dy).sqrt();
    if horiz < VERTICAL_STEP_HORIZ_MM {
        return None;
    }
    Some(dz.atan2(horiz).abs().to_degrees())
}

#[derive(Clone, Debug)]
struct Violation {
    x: f64,
    y: f64,
    slope_deg: f64,
}

/// Scan finished G-code and list extrusion segments in the curved band that exceed the cap.
pub fn slope_violations(gcode: &str, field: &DeformField, max_deg: f64) -> Vec<(f64, f64, f64)> {
    let limit = max_deg + SLOPE_CAP_TOLERANCE_DEG;
    let mut out = Vec::new();
    scan_violations(gcode, field, limit, &mut |v| out.push((v.x, v.y, v.slope_deg)));
    out
}

fn scan_violations(
    gcode: &str,
    field: &DeformField,
    limit_deg: f64,
    sink: &mut impl FnMut(Violation),
) {
    let mut position = [0.0f64; 3];
    let mut prev_extrusion: Option<[f64; 3]> = None;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with("G0") && (line.contains('X') || line.contains('Y')) {
            prev_extrusion = None;
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
        let extruding = line.starts_with("G1")
            && line.contains('E')
            && word_f(line, 'E').unwrap_or(0.0) > 1e-9;
        if !extruding {
            continue;
        }
        let point = position;
        if let Some(prev) = prev_extrusion {
            if segment_in_curved_band(field, prev, point) {
                if let Some(deg) = slope_deg(prev, point) {
                    if deg > limit_deg {
                        sink(Violation {
                            x: 0.5 * (prev[0] + point[0]),
                            y: 0.5 * (prev[1] + point[1]),
                            slope_deg: deg,
                        });
                    }
                }
            }
        }
        prev_extrusion = Some(point);
    }
}

/// Largest extrusion slope (degrees) in the curved band; `None` if no band moves.
pub fn max_curved_band_extrusion_slope_deg(gcode: &str, field: &DeformField) -> Option<f64> {
    let mut position = [0.0f64; 3];
    let mut prev_extrusion: Option<[f64; 3]> = None;
    let mut max_deg = 0.0f64;
    let mut any = false;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with("G0") && (line.contains('X') || line.contains('Y')) {
            prev_extrusion = None;
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
        let extruding = line.starts_with("G1")
            && line.contains('E')
            && word_f(line, 'E').unwrap_or(0.0) > 1e-9;
        if !extruding {
            continue;
        }
        let point = position;
        if let Some(prev) = prev_extrusion {
            if segment_in_curved_band(field, prev, point) {
                if let Some(deg) = slope_deg(prev, point) {
                    any = true;
                    max_deg = max_deg.max(deg);
                }
            }
        }
        prev_extrusion = Some(point);
    }
    any.then_some(max_deg)
}

pub fn gcode_respects_max_angle(gcode: &str, field: &DeformField) -> bool {
    slope_violations(gcode, field, field.max_angle_deg).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deform::build_deform_field;
    use crate::height_field::build_height_field;
    use crate::test_meshes::box_mesh;

    #[test]
    fn slope_scan_flags_a_steep_band_extrusion() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        // A 3 mm rise over 2 mm of travel is 56°, far above the 10° cap.
        let gcode = "G1 X3 Y5 Z9 E1\nG1 X5 Y5 Z9 E2\nG1 X7 Y5 Z12 E3\n";
        let violations = slope_violations(gcode, &field, 10.0);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].2 > 50.0);
    }

    #[test]
    fn slope_scan_accepts_a_flat_band_extrusion() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let gcode = "G1 X5 Y5 Z9.6 E0\nG1 X15 Y5 Z9.6 E1\n";
        assert!(gcode_respects_max_angle(gcode, &field));
    }
}
