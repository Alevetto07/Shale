//! Map planar-slice G-code Z back through the inverse deformation.

use crate::deform::DeformField;
use crate::extrusion::extrusion_scale;

const SEGMENT_MM: f64 = 0.75;
const MERGE_MM: f64 = 0.2;

/// Post-process G-code text: inverse Z map, segmented moves, E correction.
pub fn map_gcode_inverse(field: &DeformField, gcode: &str, nominal_layer: f64) -> String {
    let _ = nominal_layer;
    let mut absolute = true;
    let mut position = [0.0f64; 4]; // x,y,z,e
    let mut out = String::with_capacity(gcode.len() + 4096);
    out.push_str("; nonplanar curved-tops inverse map applied by shale-nonplanar\n");

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
        let extruding = word_f(line, 'E').is_some() && {
            let e = word_f(line, 'E').unwrap_or(0.0);
            if absolute {
                e > position[3]
            } else {
                e > 0.0
            }
        };

        let dx = target[0] - position[0];
        let dy = target[1] - position[1];
        let horiz = (dx * dx + dy * dy).sqrt();
        let segments = (horiz / SEGMENT_MM).ceil().max(1.0) as usize;

        let mut prev_phys = [
            position[0],
            position[1],
            field.inverse_z(position[0], position[1], position[2]),
        ];
        let mut prev_e = position[3];

        for seg in 1..=segments {
            let t = seg as f64 / segments as f64;
            let x = position[0] + dx * t;
            let y = position[1] + dy * t;
            let z_def = position[2] + (target[2] - position[2]) * t;
            let z_phys = field.inverse_z(x, y, z_def);
            if seg < segments {
                let step_horiz =
                    ((x - prev_phys[0]).powi(2) + (y - prev_phys[1]).powi(2)).sqrt();
                if step_horiz < MERGE_MM {
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
        let field = build_deform_field(grid, 2.0, 0.4, 0.2);
        for z in [15.0, 16.0, 17.0, 18.0] {
            let back = round_trip_z(&field, 0.0, 0.0, z);
            assert!(
                (back - z).abs() < 0.5,
                "round-trip at z={z}: got {back}"
            );
        }
    }

    #[test]
    fn map_preserves_header_comments() {
        let mesh = box_mesh(10.0, 10.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2);
        let src = ";LAYER:1\nG1 X10 Y10 Z9.8 E1.0 F1800\n";
        let out = map_gcode_inverse(&field, src, 0.2);
        assert!(out.contains("nonplanar"));
        assert!(out.contains("Z"));
    }
}