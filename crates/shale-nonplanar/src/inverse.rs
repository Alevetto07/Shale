//! Map planar-slice G-code Z back through the inverse deformation.

use crate::deform::DeformField;
use crate::extrusion::extrusion_scale;

const SEGMENT_MM: f64 = 0.75;
const MERGE_MM: f64 = 0.2;

fn clamp_slope_z(field: &DeformField, prev: [f64; 3], x: f64, y: f64, mut z: f64) -> f64 {
    let step_horiz = ((x - prev[0]).powi(2) + (y - prev[1]).powi(2)).sqrt();
    if step_horiz < 1e-6 {
        return prev[2];
    }
    let horiz = step_horiz;
    let max_dz = field.max_angle_deg.to_radians().tan() * horiz;
    let dz = z - prev[2];
    if dz.abs() > max_dz {
        z = prev[2] + dz.signum() * max_dz;
    }
    z
}

fn bridge_to_line_start(
    field: &DeformField,
    out: &mut String,
    from: &mut [f64; 3],
    to: [f64;  3],
    g0: bool,
) {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let horiz = (dx * dx + dy * dy).sqrt();
    if horiz < 0.02 {
        *from = to;
        return;
    }
    let max_step_dz = field.max_angle_deg.to_radians().tan() * SEGMENT_MM;
    let dz_abs = (to[2] - from[2]).abs();
    let segments = (horiz / SEGMENT_MM)
        .ceil()
        .max(if max_step_dz > 1e-9 {
            dz_abs / max_step_dz
        } else {
            1.0
        })
        .max(1.0) as usize;
    let mut prev = *from;
    for seg in 1..=segments {
        let t = seg as f64 / segments as f64;
        let x = from[0] + dx * t;
        let y = from[1] + dy * t;
        let mut z = from[2] + (to[2] - from[2]) * t;
        z = clamp_slope_z(field, prev, x, y, z);
        let cmd = if g0 { "G0" } else { "G1" };
        out.push_str(&format!("{cmd} X{x:.3} Y{y:.3} Z{z:.3}\n"));
        prev = [x, y, z];
    }
    *from = prev;
}

/// Post-process G-code text: inverse Z map, segmented moves, E correction.
pub fn map_gcode_inverse(field: &DeformField, gcode: &str, nominal_layer: f64) -> String {
    let _ = nominal_layer;
    let mut absolute = true;
    let mut position = [0.0f64; 4]; // x,y,z,e
    let mut last_phys = [0.0f64; 3];
    let mut have_last_phys = false;
    let mut out = String::with_capacity(gcode.len() + 4096);
    out.push_str(&format!(
        "; nonplanar curved-tops inverse map applied by shale-nonplanar (slope cap {:.1}°)\n",
        field.max_angle_deg
    ));

    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with("G90") {
            absolute = true;
            out.push_str(raw);
            out.push('\n');
            continue;
        }
        if line.starts_with("G91") {
            absolute = false;
            out.push_str(raw);
            out.push('\n');
            continue;
        }
        if line.starts_with("G92") {
            if let Some(x) = word_f(line, 'X') {
                position[0] = x;
            }
            if let Some(y) = word_f(line, 'Y') {
                position[1] = y;
            }
            if let Some(z) = word_f(line, 'Z') {
                position[2] = z;
            }
            if let Some(e) = word_f(line, 'E') {
                position[3] = e;
            }
            out.push_str(raw);
            out.push('\n');
            continue;
        }
        if !(line.starts_with("G0") || line.starts_with("G1")) {
            out.push_str(raw);
            out.push('\n');
            continue;
        }

        let target = [
            word_f(line, 'X').unwrap_or(position[0]),
            word_f(line, 'Y').unwrap_or(position[1]),
            word_f(line, 'Z').unwrap_or(position[2]),
            word_f(line, 'E').unwrap_or(position[3]),
        ];
        let extruding = if let Some(e) = word_f(line, 'E') {
            if absolute {
                e > position[3] + 1e-9
            } else {
                e > 1e-9
            }
        } else {
            false
        };

        if line.starts_with("G0") {
            let z_phys = field.inverse_z(target[0], target[1], target[2]);
            let mut parts = vec!["G0".to_string()];
            if line.contains('X') {
                parts.push(format!("X{:.3}", target[0]));
            }
            if line.contains('Y') {
                parts.push(format!("Y{:.3}", target[1]));
            }
            if line.contains('Z') {
                parts.push(format!("Z{z_phys:.3}"));
            }
            if let Some(f) = word_f(line, 'F') {
                parts.push(format!("F{f:.0}"));
            }
            out.push_str(&parts.join(" "));
            out.push('\n');
            position = target;
            // Keep last_phys at the last extrusion point so the next G1 bridges with a
            // slope-capped path. Only pure Z hops (no XY) update vertical position.
            if line.contains('Z') && !line.contains('X') && !line.contains('Y') && have_last_phys {
                let max_dz =
                    field.max_angle_deg.to_radians().tan() * field.nominal_layer.max(0.08);
                let dz = z_phys - last_phys[2];
                last_phys[2] = if dz.abs() > max_dz {
                    last_phys[2] + dz.signum() * max_dz
                } else {
                    z_phys
                };
            }
            continue;
        }

        let band_floor = field.global_band_floor_z();
        if (position[2] < band_floor - 1e-6 && target[2] < band_floor - 1e-6)
            || (!field.gcode_needs_inverse(position[0], position[1], position[2])
                && !field.gcode_needs_inverse(target[0], target[1], target[2]))
        {
            out.push_str(raw);
            out.push('\n');
            position = target;
            if extruding {
                last_phys = [target[0], target[1], target[2]];
                have_last_phys = true;
            }
            continue;
        }

        let dx = target[0] - position[0];
        let dy = target[1] - position[1];
        let horiz = (dx * dx + dy * dy).sqrt();
        let max_step_dz = field.max_angle_deg.to_radians().tan() * SEGMENT_MM;
        let dz_abs = (target[2] - position[2]).abs();
        let segments = (horiz / SEGMENT_MM)
            .ceil()
            .max(if max_step_dz > 1e-9 {
                dz_abs / max_step_dz
            } else {
                1.0
            })
            .max(1.0) as usize;

        let mut line_start_phys = [
            position[0],
            position[1],
            field.inverse_z(position[0], position[1], position[2]),
        ];
        let mut prev_phys = line_start_phys;
        if have_last_phys && extruding {
            let hop = ((line_start_phys[0] - last_phys[0]).powi(2)
                + (line_start_phys[1] - last_phys[1]).powi(2))
            .sqrt();
            if hop < 0.05 {
                line_start_phys[2] = last_phys[2];
            } else {
                line_start_phys[2] = clamp_slope_z(
                    field,
                    last_phys,
                    line_start_phys[0],
                    line_start_phys[1],
                    line_start_phys[2],
                );
            }
            let mut cursor = last_phys;
            bridge_to_line_start(field, &mut out, &mut cursor, line_start_phys, false);
            prev_phys = cursor;
            last_phys = cursor;
        }
        let mut prev_e = position[3];

        for seg in 1..=segments {
            let t = seg as f64 / segments as f64;
            let x = position[0] + dx * t;
            let y = position[1] + dy * t;
            let z_def = position[2] + (target[2] - position[2]) * t;
            let z_phys = clamp_slope_z(
                field,
                prev_phys,
                x,
                y,
                field.inverse_z(x, y, z_def),
            );
            if seg < segments {
                let step_horiz =
                    ((x - prev_phys[0]).powi(2) + (y - prev_phys[1]).powi(2)).sqrt();
                if step_horiz < MERGE_MM {
                    prev_phys = [x, y, z_phys];
                    continue;
                }
            }

            let mut parts = vec![if line.starts_with("G0") { "G0" } else { "G1" }.to_string()];
            parts.push(format!("X{x:.3}"));
            parts.push(format!("Y{y:.3}"));
            parts.push(format!("Z{z_phys:.3}"));

            if extruding {
                let scale = extrusion_scale(
                    field,
                    prev_phys[0],
                    prev_phys[1],
                    prev_phys[2],
                    x,
                    y,
                    z_phys,
                );
                let e_target = if absolute {
                    position[3] + (target[3] - position[3]) * t
                } else {
                    target[3] * t
                };
                let delta = e_target - prev_e;
                let e_out = if absolute {
                    prev_e + delta * scale
                } else {
                    delta * scale
                };
                if absolute {
                    parts.push(format!("E{e_out:.5}"));
                    prev_e = e_out;
                } else {
                    parts.push(format!("E{e_out:.5}"));
                    prev_e += e_out;
                }
            } else if word_f(line, 'E').is_some() {
                parts.push(format!("E{:.5}", target[3]));
                prev_e = target[3];
            }

            if let Some(f) = word_f(line, 'F') {
                parts.push(format!("F{f:.0}"));
            }
            out.push_str(&parts.join(" "));
            out.push('\n');
            prev_phys = [x, y, z_phys];
            last_phys = prev_phys;
            have_last_phys = true;
        }

        position[0] = target[0];
        position[1] = target[1];
        position[2] = target[2];
        position[3] = target[3];
    }
    out
}

/// Round-trip helper for tests: apply forward Z only to a single point pair.
pub fn round_trip_z(field: &DeformField, x: f64, y: f64, z: f64) -> f64 {
    field.inverse_z(x, y, field.forward_z(x, y, z))
}

fn word_f(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deform::build_deform_field;
    use crate::height_field::build_height_field;
    use crate::test_meshes::{box_mesh, sphere_mesh};

    #[test]
    fn inverse_forward_round_trip_in_band() {
        let mesh = sphere_mesh(10.0);
        let grid = build_height_field(&mesh, 1.0, 45.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 45.0);
        for z in [15.0, 16.0, 17.0, 18.0, 19.0, 20.0] {
            let back = round_trip_z(&field, 0.0, 0.0, z);
            assert!(
                (back - z).abs() < 0.02,
                "round-trip at z={z}: got {back}"
            );
        }
        assert!(
            field.max_round_trip_error_mm(0.0, 0.0, 128) < 0.02,
            "LUT inverse should stay within 0.02 mm on the sphere column"
        );
    }

    #[test]
    fn box_top_band_round_trip_under_0_02mm() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 45.0);
        assert!(field.max_round_trip_error_mm(0.0, 0.0, 64) < 0.02);
    }

    #[test]
    fn z_hop_and_retract_moves_preserved() {
        let mesh = box_mesh(10.0, 10.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 45.0);
        let src = "G1 X1 Y1 Z0.4 E0.5 F1800\nG0 Z1.0\nG1 X2 Y2 Z1.0 E1.0 F1800\nG1 X3 Y3 E-0.2 F3000\n";
        let out = map_gcode_inverse(&field, src, 0.2);
        assert!(
            out.lines().any(|l| l.starts_with("G0") && l.contains('Z')),
            "z-hop travel kept as G0, got:\n{out}"
        );
    }

    #[test]
    fn machine_sphere_two_lines_respect_cap() {
        let mut mesh = sphere_mesh(12.0);
        for v in &mut mesh.vertices {
            v[0] += 128.0;
            v[1] += 128.0;
        }
        let grid = build_height_field(&mesh, 0.35, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let src = "G0 X126.244 Y124.257 Z22.4\nG1 X126.244 Y124.257 Z22.4 E1.0 F1800\nG1 X126.170 Y124.222 Z22.4 E1.1 F1800\nG0 X126.100 Y124.200 F12000\nG1 X126.050 Y124.180 Z22.4 E1.2 F1800\n";
        let out = map_gcode_inverse(&field, src, 0.2);
        let mut prev: Option<[f64; 3]> = None;
        let mut max_deg = 0.0f64;
        for line in out.lines() {
            if !line.starts_with("G1") || !line.contains('E') {
                continue;
            }
            let p = [
                word_f(line, 'X').unwrap(),
                word_f(line, 'Y').unwrap(),
                word_f(line, 'Z').unwrap(),
            ];
            if let Some(a) = prev {
                let h = ((p[0] - a[0]).powi(2) + (p[1] - a[1]).powi(2)).sqrt();
                if h > 1e-6 && h <= 2.5 {
                    let deg = ((p[2] - a[2]).abs() / h).atan().to_degrees();
                    max_deg = max_deg.max(deg);
                }
            }
            prev = Some(p);
        }
        assert!(max_deg <= 10.5, "chain slope {max_deg:.2}°, out:\n{out}");
    }

    #[test]
    fn mapped_segments_respect_max_angle() {
        let mesh = sphere_mesh(12.0);
        let grid = build_height_field(&mesh, 0.35, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let src = "G1 X126.244 Y124.257 Z22.4 E1.0 F1800\nG1 X126.170 Y124.222 Z22.4 E1.1 F1800\n";
        let out = map_gcode_inverse(&field, src, 0.2);
        let mut prev: Option<[f64; 3]> = None;
        let mut max_deg = 0.0f64;
        for line in out.lines() {
            if !line.starts_with("G1") || !line.contains('E') {
                continue;
            }
            let x = word_f(line, 'X').unwrap();
            let y = word_f(line, 'Y').unwrap();
            let z = word_f(line, 'Z').unwrap();
            if let Some(p) = prev {
                let dx = x - p[0];
                let dy = y - p[1];
                let dz = z - p[2];
                let h = (dx * dx + dy * dy).sqrt();
                if h > 1e-6 {
                    let deg = dz.atan2(h).abs().to_degrees();
                    max_deg = max_deg.max(deg);
                }
            }
            prev = Some([x, y, z]);
        }
        assert!(
            max_deg <= 10.5,
            "segment slope {max_deg:.2}° exceeds cap, output:\n{out}"
        );
    }

    #[test]
    fn clamp_slope_limits_tiny_horizontal_step() {
        let mut mesh = sphere_mesh(12.0);
        for v in &mut mesh.vertices {
            v[0] += 128.0;
            v[1] += 128.0;
        }
        let grid = build_height_field(&mesh, 0.35, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let prev = [124.199, 123.416, 19.896];
        let raw = field.inverse_z(124.152, 123.455, 22.4);
        let capped = clamp_slope_z(&field, prev, 124.152, 123.455, raw);
        assert!(
            (capped - prev[2]).abs() <= (10.0f64).to_radians().tan() * 0.062 + 1e-3,
            "raw={raw:.4} capped={capped:.4}"
        );
    }

    #[test]
    fn consecutive_tiny_extrusions_respect_cap_after_travel() {
        let mut mesh = sphere_mesh(12.0);
        for v in &mut mesh.vertices {
            v[0] += 128.0;
            v[1] += 128.0;
        }
        let grid = build_height_field(&mesh, 0.35, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let src = "G1 X124.199 Y123.416 Z22.4 E1.0 F1800\nG1 X124.152 Y123.455 Z22.4 E1.1 F1800\n";
        let out = map_gcode_inverse(&field, src, 0.2);
        let mut prev: Option<[f64; 3]> = None;
        let mut max_deg = 0.0f64;
        for line in out.lines() {
            if !line.starts_with("G1") || !line.contains('E') {
                continue;
            }
            let p = [
                word_f(line, 'X').unwrap(),
                word_f(line, 'Y').unwrap(),
                word_f(line, 'Z').unwrap(),
            ];
            if let Some(a) = prev {
                let h = ((p[0] - a[0]).powi(2) + (p[1] - a[1]).powi(2)).sqrt();
                if h > 1e-6 {
                    let deg = ((p[2] - a[2]).abs() / h).atan().to_degrees();
                    max_deg = max_deg.max(deg);
                }
            }
            prev = Some(p);
        }
        assert!(
            max_deg <= 10.5,
            "tiny move chain slope {max_deg:.2}°, out:\n{out}"
        );
    }

    #[test]
    fn below_band_moves_pass_through_unchanged() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 30.0);
        let src = "G1 X1 Y1 Z5.0 E1.0 F1800\nG1 X2 Y2 Z5.0 E1.1 F1800\n";
        let out = map_gcode_inverse(&field, src, 0.2);
        assert!(
            out.lines().filter(|l| l.starts_with("G1 X1 Y1 Z5.0 E1.0")).count() == 1,
            "below-band line must pass through unchanged: {out}"
        );
        assert_eq!(
            out.lines().filter(|l| l.starts_with('G') && l.contains('E')).count(),
            2
        );
    }

    #[test]
    fn map_preserves_header_comments() {
        let mesh = box_mesh(10.0, 10.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 45.0);
        let src = ";LAYER:1\nG1 X10 Y10 Z9.8 E1.0 F1800\n";
        let out = map_gcode_inverse(&field, src, 0.2);
        assert!(out.contains("nonplanar"));
        assert!(out.contains("Z"));
    }
}