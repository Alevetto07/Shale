//! Hard guarantee: extrusion slopes in the curved band stay within `max_angle_deg`.

use crate::config::NonplanarReport;
use crate::deform::DeformField;
use crate::inverse::map_gcode_inverse;

/// Ignore sub-millimetre horizontal hops when chaining extrusions (layer Z-steps at fixed XY).
const VERTICAL_STEP_HORIZ_MM: f64 = 0.05;
/// Reporting / validation slack above `max_angle_deg` (inverse quantization, hard clamp).
pub const SLOPE_CAP_TOLERANCE_DEG: f64 = 0.5;

pub(crate) fn dampen_cell(field: &mut DeformField, x: f64, y: f64, factor: f64) {
    let fx = (x - field.grid.origin[0]) / field.grid.step;
    let fy = (y - field.grid.origin[1]) / field.grid.step;
    let ix = fx.round().clamp(0.0, field.grid.width as f64 - 1.0) as usize;
    let iy = fy.round().clamp(0.0, field.grid.height as f64 - 1.0) as usize;
    let i = iy * field.grid.width + ix;
    if let Some(s) = field.island_strength.get_mut(i) {
        *s *= factor;
    }
}

fn word_f(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn segment_in_curved_band(field: &DeformField, a: [f64; 3], b: [f64; 3]) -> bool {
    point_in_curved_band_geom(field, a) && point_in_curved_band_geom(field, b)
}

fn slope_deg(a: [f64; 3], b: [f64; 3]) -> Option<f64> {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let dz = b[2] - a[2];
    let horiz = (dx * dx + dy * dy).sqrt();
    if horiz < VERTICAL_STEP_HORIZ_MM {
        return None;
    }
    if horiz < 1e-9 {
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

/// Re-map from planar G-code, dampening islands until slopes fit or planar fallback.
pub fn enforce_slope_safety(
    field: &mut DeformField,
    planar_gcode: &str,
    nominal_layer: f64,
    report: &mut NonplanarReport,
) -> String {
    const MAX_ITERS: usize = 16;
    let limit = field.max_angle_deg;
    let mut mapped = map_gcode_inverse(field, planar_gcode, nominal_layer);
    for pass in 0..MAX_ITERS {
        let violations = slope_violations(&mapped, field, limit);
        if violations.is_empty() {
            if pass > 0 {
                report
                    .notes
                    .push(format!("slope safety converged after {pass} dampening pass(es)"));
            }
            break;
        }
        let mut worst = 0.0f64;
        for (x, y, deg) in &violations {
            dampen_cell(field, *x, *y, 0.72);
            report.islands_flattened += 1;
            worst = worst.max(*deg);
            let _ = (x, y);
        }
        report.notes.push(format!(
            "slope safety pass {}: {} violation(s), worst {:.2}°",
            pass + 1,
            violations.len(),
            worst
        ));
        mapped = map_gcode_inverse(field, planar_gcode, nominal_layer);
        if field.island_strength.iter().all(|&s| s < 0.12) {
            report.fallback_to_planar = true;
            report.notes.push(
                "slope safety: deformation greatly reduced on all islands".into(),
            );
            break;
        }
    }
    for _ in 0..12 {
        let next = hard_clamp_band_extrusions(&mapped, field);
        if next == mapped {
            break;
        }
        mapped = next;
    }
    if !gcode_respects_max_angle(&mapped, field) {
        report.fallback_to_planar = true;
        report.notes.push(format!(
            "slope safety: {:.2}° cap still exceeded after dampening and hard clamp",
            limit
        ));
    } else {
        report
            .notes
            .push("slope safety: hard clamp applied to curved-band extrusions".into());
    }
    mapped
}

/// Final guarantee: clamp extrusion Z steps in the curved band on finished G-code.
fn point_in_curved_band_geom(field: &DeformField, p: [f64; 3]) -> bool {
    p[2] + 1e-6 >= field.global_band_floor_z()
        && field
            .grid
            .sample_band_start(p[0], p[1])
            .is_some_and(|z_band| p[2] > z_band + 1e-6)
}

fn hard_clamp_band_extrusions(gcode: &str, field: &DeformField) -> String {
    let max_tan = field.max_angle_deg.to_radians().tan();
    let mut position = [0.0f64; 3];
    let mut last_extrusion: Option<[f64; 3]> = None;
    let mut out = String::with_capacity(gcode.len());
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with(";LAYER:") {
            // Layer changes are not travel; keep the extrusion chain for cross-layer caps.
        } else if line.starts_with("G0") && (line.contains('X') || line.contains('Y')) {
            last_extrusion = None;
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
        if extruding {
            let mut target = position;
            let mut clamped = false;
            if point_in_curved_band_geom(field, target) {
                if let Some(prev) = last_extrusion {
                    let horiz =
                        ((target[0] - prev[0]).powi(2) + (target[1] - prev[1]).powi(2)).sqrt();
                    if horiz >= VERTICAL_STEP_HORIZ_MM {
                        let max_dz = max_tan * horiz;
                        let dz = target[2] - prev[2];
                        if dz.abs() > max_dz + 1e-9 {
                            target[2] = prev[2] + dz.signum() * max_dz;
                            clamped = true;
                        }
                    }
                }
            }
            if clamped {
                let mut parts = vec!["G1".to_string()];
                if line.contains('X') {
                    parts.push(format!("X{:.3}", target[0]));
                }
                if line.contains('Y') {
                    parts.push(format!("Y{:.3}", target[1]));
                }
                parts.push(format!("Z{:.3}", target[2]));
                if let Some(e) = word_f(line, 'E') {
                    parts.push(format!("E{e:.5}"));
                }
                if let Some(f) = word_f(line, 'F') {
                    parts.push(format!("F{f:.0}"));
                }
                out.push_str(&parts.join(" "));
                out.push('\n');
            } else {
                out.push_str(raw);
                out.push('\n');
            }
            last_extrusion = Some(target);
            continue;
        }
        out.push_str(raw);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod equiv_tests {
    use super::*;
    use crate::deform::build_deform_field;
    use crate::equivalence::check_planar_equivalence;
    use crate::height_field::build_height_field;
    use crate::test_meshes::box_mesh;

    #[test]
    fn slope_safety_keeps_planar_shell() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 10.0, 1.6);
        let mut field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let planar = "G1 X5 Y5 Z2 E0\nG1 X15 Y5 Z2 E1\nG0 X15 Y15 Z9\nG1 X15 Y15 Z9 E2\nG1 X5 Y15 Z9.2 E3\n";
        let mut report = crate::config::NonplanarReport::default();
        let out = enforce_slope_safety(&mut field, planar, 0.2, &mut report);
        let floor = field.global_band_floor_z();
        let eq = check_planar_equivalence(planar, &out, floor);
        assert!(eq.ok, "{eq:?} notes={:?}", report.notes);
    }
}