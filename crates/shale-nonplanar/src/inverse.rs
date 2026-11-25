//! Map planar-slice G-code Z back through the inverse deformation.

use crate::config::InverseMapStats;
use crate::deform::DeformField;
use crate::extrusion::extrusion_scale;

const SEGMENT_MM: f64 = 0.75;
const MERGE_MM: f64 = 0.2;

/// Largest Z correction the inverse map may make on its own.
///
/// Anything bigger means the deformation asked for a move the toolhead cannot
/// follow; the map leaves that move at its true inverse and the post-pass
/// verification rejects the file (planar fallback) instead of clamping silently.
pub const MAX_INVERSE_CLAMP_MM: f64 = 0.02;

fn clamp_slope_z(
    field: &DeformField,
    prev: [f64; 3],
    x: f64,
    y: f64,
    z: f64,
    stats: &mut InverseMapStats,
    count_excess: bool,
) -> f64 {
    let step_horiz = ((x - prev[0]).powi(2) + (y - prev[1]).powi(2)).sqrt();
    if step_horiz < 1e-6 {
        // No horizontal travel means no slope to cap. These are the slicer's Z
        // hops and layer changes; the shear translates a vertical move unchanged,
        // so treating it as an infinite slope rejected three of them on the demo
        // lid ("3 inverse-map move(s) needed more than the 0.02 mm slope-clamp
        // budget") for a 0.167 mm hop the printer makes on every layer.
        return z;
    }
    let max_dz = field.max_angle_deg.to_radians().tan() * step_horiz;
    let dz = z - prev[2];
    if dz.abs() > max_dz + 1e-9 {
        let bound = prev[2] + dz.signum() * max_dz;
        let correction = (bound - z).abs();
        if correction <= MAX_INVERSE_CLAMP_MM + 1e-9 {
            stats.clamps += 1;
            stats.largest_clamp_mm = stats.largest_clamp_mm.max(correction);
            return bound;
        }
        // A layer-change join between two extrusions is not a surface. The
        // slicer often steps a full layer across a few tenths of a millimetre,
        // which is steeper than the nozzle cap even when the surface itself is
        // flat. Counting that join rejected a flat deck whose extrusions all
        // sat on the cap. Only an extruding segment can fail the file.
        if count_excess {
            stats.unclamped_excess += 1;
            if std::env::var_os("SHALE_TRACE").is_some() {
                let slope = (dz.abs() / step_horiz).atan().to_degrees();
                eprintln!(
                    "inverse excess {correction:.4} mm over {step_horiz:.3} mm ({slope:.2}°) at {x:.2},{y:.2}"
                );
            }
        }
    }
    z
}

fn bridge_to_line_start(
    field: &DeformField,
    out: &mut String,
    from: &mut [f64; 3],
    to: [f64; 3],
    z_def: [f64; 2],
    stats: &mut InverseMapStats,
) {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let horiz = (dx * dx + dy * dy).sqrt();
    if horiz < 0.02 {
        *from = to;
        return;
    }
    // Interpolate in deformed Z and map each sample. A straight line in physical
    // Z between two rim points cuts under a dome whose middle sits higher.
    let max_step_dz = field.max_angle_deg.to_radians().tan() * SEGMENT_MM;
    let dz_abs = (z_def[1] - z_def[0]).abs();
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
        let z_def = z_def[0] + (z_def[1] - z_def[0]) * t;
        let mut z = match field.local_band_floor_z(x, y) {
            Some(zb) if z_def >= zb - 1e-6 => field.inverse_z(x, y, z_def),
            _ => z_def,
        };
        z = clamp_slope_z(field, prev, x, y, z, stats, false);
        out.push_str(&format!("G1 X{x:.3} Y{y:.3} Z{z:.3}\n"));
        prev = [x, y, z];
    }
    *from = prev;
}

/// Post-process G-code text: inverse Z map, segmented moves, E correction.
///
/// Thin wrapper over [`map_gcode_inverse_counted`] for callers that do not need
/// the correction statistics.
pub fn map_gcode_inverse(field: &DeformField, gcode: &str, nominal_layer: f64) -> String {
    map_gcode_inverse_counted(field, gcode, nominal_layer).0
}

/// Inverse map plus the count of slope corrections it had to make.
///
/// Corrections are capped at [`MAX_INVERSE_CLAMP_MM`] per move; anything larger
/// is left in the G-code and counted in `unclamped_excess` so the post-pass can
/// reject the file rather than ship a silently straightened path.
pub fn map_gcode_inverse_counted(
    field: &DeformField,
    gcode: &str,
    nominal_layer: f64,
) -> (String, InverseMapStats) {
    let _ = nominal_layer;
    let mut absolute = true;
    // G90/G91 move X, Y and Z. `absolute` stays the extrusion mode the rest of
    // this function already used; mixing the two made a relative wipe (`G91` /
    // `G1 X-35`) look like a jump to X = -35.
    let mut absolute_xyz = true;
    let mut position = [0.0f64; 4]; // x,y,z,e
    let mut last_phys = [0.0f64; 3];
    let mut last_z_def = 0.0f64;
    let mut have_last_phys = false;
    let mut stats = InverseMapStats::default();
    let mut out = String::with_capacity(gcode.len() + 4096);
    out.push_str(&format!(
        "; nonplanar curved-tops inverse map applied by shale-nonplanar (slope cap {:.1}°)\n",
        field.max_angle_deg
    ));

    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with("G90") {
            absolute = true;
            absolute_xyz = true;
            out.push_str(raw);
            out.push('\n');
            continue;
        }
        if line.starts_with("G91") {
            absolute = false;
            absolute_xyz = false;
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

        let placed = |index: usize, letter: char| -> f64 {
            match word_f(line, letter) {
                Some(value) if absolute_xyz => value,
                Some(value) => position[index] + value,
                None => position[index],
            }
        };
        let target = [
            placed(0, 'X'),
            placed(1, 'Y'),
            placed(2, 'Z'),
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

        // A pure Z hop has no horizontal slope. An XY travel is a real path:
        // mapping only its endpoint and then bridging the next extrusion with a
        // straight physical chord cuts through a rim-referenced dome, because the
        // physical surface is higher in the middle than at the two ends. Those
        // travels fall through and are segmented on the inverse, like a G1.
        if line.starts_with("G0") && !line.contains('X') && !line.contains('Y') {
            let z_phys = field.inverse_z(target[0], target[1], target[2]);
            let mut parts = vec!["G0".to_string()];
            if line.contains('Z') {
                parts.push(format!("Z{z_phys:.3}"));
            }
            if let Some(f) = word_f(line, 'F') {
                parts.push(format!("F{f:.0}"));
            }
            out.push_str(&parts.join(" "));
            out.push('\n');
            position = target;
            if line.contains('Z') && have_last_phys {
                let max_dz = field.max_angle_deg.to_radians().tan() * field.nominal_layer.max(0.08);
                let dz = z_phys - last_phys[2];
                last_phys[2] = if dz.abs() > max_dz {
                    last_phys[2] + dz.signum() * max_dz
                } else {
                    z_phys
                };
                last_z_def = target[2];
            }
            continue;
        }

        let below_local_floor = |x: f64, y: f64, z: f64| {
            field
                .local_band_floor_z(x, y)
                .is_none_or(|zb| z < zb - 1e-6)
        };
        let midpoint_needs = || {
            field.gcode_needs_inverse(
                (position[0] + target[0]) * 0.5,
                (position[1] + target[1]) * 0.5,
                (position[2] + target[2]) * 0.5,
            )
        };
        if (below_local_floor(position[0], position[1], position[2])
            && below_local_floor(target[0], target[1], target[2])
            && below_local_floor(
                (position[0] + target[0]) * 0.5,
                (position[1] + target[1]) * 0.5,
                (position[2] + target[2]) * 0.5,
            ))
            || (!field.gcode_needs_inverse(position[0], position[1], position[2])
                && !field.gcode_needs_inverse(target[0], target[1], target[2])
                && !midpoint_needs())
        {
            out.push_str(raw);
            out.push('\n');
            position = target;
            if extruding {
                last_phys = [target[0], target[1], target[2]];
                last_z_def = target[2];
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
            match field.local_band_floor_z(position[0], position[1]) {
                Some(zb) if position[2] >= zb - 1e-6 => {
                    field.inverse_z(position[0], position[1], position[2])
                }
                _ => position[2],
            },
        ];
        let mut prev_phys = line_start_phys;
        if have_last_phys {
            let hop = ((line_start_phys[0] - last_phys[0]).powi(2)
                + (line_start_phys[1] - last_phys[1]).powi(2))
            .sqrt();
            // The previous line may have left the nozzle a clamp away from this
            // point's raw inverse. Recomputing the inverse here and stepping from
            // it skips that correction, so a short following move exceeds the cap.
            if hop < 0.05 {
                line_start_phys[2] = last_phys[2];
            } else {
                line_start_phys[2] = clamp_slope_z(
                    field,
                    last_phys,
                    line_start_phys[0],
                    line_start_phys[1],
                    line_start_phys[2],
                    &mut stats,
                    false,
                );
            }
            if extruding {
                let mut cursor = last_phys;
                bridge_to_line_start(
                    field,
                    &mut out,
                    &mut cursor,
                    line_start_phys,
                    [last_z_def, position[2]],
                    &mut stats,
                );
                prev_phys = cursor;
                last_phys = cursor;
                last_z_def = position[2];
            } else {
                prev_phys = line_start_phys;
            }
        }
        let mut prev_e = position[3];
        // At the start of a file there is no previous extrusion to hold the cap
        // against: the printer's real position is whatever the start-up script left
        // it at, so clamping the first move against Z=0 would count a dozen phantom
        // corrections and push the whole job to the planar fallback.
        let mut anchored = have_last_phys;

        for seg in 1..=segments {
            let t = seg as f64 / segments as f64;
            let x = position[0] + dx * t;
            let y = position[1] + dy * t;
            let z_def = position[2] + (target[2] - position[2]) * t;
            let z_phys = match field.local_band_floor_z(x, y) {
                Some(zb) if z_def >= zb - 1e-6 => {
                    let raw = field.inverse_z(x, y, z_def);
                    if anchored {
                        // Only an extruding segment can fail the file. A travel
                        // that follows the inverse is still slope-capped when the
                        // correction fits the budget, but a combing move's own
                        // step is not a surface and must not force the fallback.
                        clamp_slope_z(field, prev_phys, x, y, raw, &mut stats, extruding)
                    } else {
                        anchored = true;
                        raw
                    }
                }
                _ => z_def,
            };
            if seg < segments {
                let step_horiz = ((x - prev_phys[0]).powi(2) + (y - prev_phys[1]).powi(2)).sqrt();
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
            last_z_def = z_def;
            have_last_phys = true;
        }

        position[0] = target[0];
        position[1] = target[1];
        position[2] = target[2];
        position[3] = target[3];
    }
    (out, stats)
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
            assert!((back - z).abs() < 0.02, "round-trip at z={z}: got {back}");
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
        let src =
            "G1 X1 Y1 Z0.4 E0.5 F1800\nG0 Z1.0\nG1 X2 Y2 Z1.0 E1.0 F1800\nG1 X3 Y3 E-0.2 F3000\n";
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

    /// A correction the field did not ask for is never silently applied: past the
    /// 0.02 mm budget the Z is left alone and the excess is counted so the verifier
    /// can reject the file.
    #[test]
    fn clamp_budget_refuses_a_large_correction_and_counts_it() {
        let mut mesh = sphere_mesh(12.0);
        for v in &mut mesh.vertices {
            v[0] += 128.0;
            v[1] += 128.0;
        }
        let grid = build_height_field(&mesh, 0.35, 10.0, 1.6);
        let field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let prev = [124.199, 123.416, 19.896];
        let raw = field.inverse_z(124.152, 123.455, 22.4);
        let mut stats = InverseMapStats::default();
        let z = clamp_slope_z(&field, prev, 124.152, 123.455, raw, &mut stats, true);
        let step = (10.0f64).to_radians().tan() * 0.062;
        assert!(
            (raw - prev[2]).abs() > step + MAX_INVERSE_CLAMP_MM,
            "fixture must ask for more than the clamp budget: raw={raw:.4}"
        );
        assert_eq!(z, raw, "a too-large correction must not be applied");
        assert_eq!(stats.clamps, 0);
        assert_eq!(stats.unclamped_excess, 1);
        assert!(stats.largest_clamp_mm == 0.0);
    }

    /// Inside the budget the correction is applied and reported.
    #[test]
    fn clamp_budget_applies_a_small_correction_and_counts_it() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 0.5, 10.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 10.0);
        let prev = [10.0, 10.0, 11.0];
        let step = (10.0f64).to_radians().tan() * 0.5;
        let mut stats = InverseMapStats::default();
        let z = clamp_slope_z(
            &field,
            prev,
            10.5,
            10.0,
            prev[2] + step + 0.01,
            &mut stats,
            true,
        );
        assert!(
            (z - (prev[2] + step)).abs() < 1e-9,
            "the move should be capped at the slope limit: {z:.4}"
        );
        assert_eq!(stats.clamps, 1);
        assert!((stats.largest_clamp_mm - 0.01).abs() < 1e-9);
        assert_eq!(stats.unclamped_excess, 0);
    }

    /// The production path: the field is limited *before* slicing, so the mapped
    /// G-code already respects the cap and the map has nothing to clamp.
    #[test]
    fn limited_field_maps_consecutive_tiny_extrusions_inside_the_cap() {
        let mut mesh = sphere_mesh(12.0);
        for v in &mut mesh.vertices {
            v[0] += 128.0;
            v[1] += 128.0;
        }
        let grid = build_height_field(&mesh, 0.35, 10.0, 1.6);
        let mut field = build_deform_field(grid, 1.6, 0.4, 0.2, 10.0);
        let limiting = crate::safety::limit_field_slopes(&mut field, 0.2);
        assert!(
            limiting.converged
                && limiting.worst_after_deg <= limiting.cap_deg + crate::SLOPE_CAP_TOLERANCE_DEG,
            "fixture field must limit to the cap: {limiting:?}"
        );
        let src = "G1 X124.199 Y123.416 Z22.4 E1.0 F1800\nG1 X124.152 Y123.455 Z22.4 E1.1 F1800\n";
        let (out, stats) = map_gcode_inverse_counted(&field, src, 0.2);
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
            "a pre-limited field must not need a clamp (saw {max_deg:.2} deg, {stats:?})\n{out}"
        );
        assert_eq!(
            stats.unclamped_excess, 0,
            "the map must not have to refuse corrections: {stats:?}"
        );
        assert!(stats.largest_clamp_mm <= MAX_INVERSE_CLAMP_MM, "{stats:?}");
    }

    #[test]
    fn below_band_moves_pass_through_unchanged() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 30.0);
        let src = "G1 X1 Y1 Z5.0 E1.0 F1800\nG1 X2 Y2 Z5.0 E1.1 F1800\n";
        let out = map_gcode_inverse(&field, src, 0.2);
        assert!(
            out.lines()
                .filter(|l| l.starts_with("G1 X1 Y1 Z5.0 E1.0"))
                .count()
                == 1,
            "below-band line must pass through unchanged: {out}"
        );
        assert_eq!(
            out.lines()
                .filter(|l| l.starts_with('G') && l.contains('E'))
                .count(),
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
