//! Curved-move accounting shared by the CLI report and the desktop preview.
//!
//! A curved move is an extruding `G1` whose slope above the bed exceeds
//! [`CURVED_EXTRUSION_MIN_SLOPE_DEG`]. That is the figure quoted for the demo
//! lid (about 134,862 moves on the A1). Planar layers only change Z between
//! layers, so they stay under the threshold.

/// Extrusions steeper than this, in degrees above horizontal, count as curved.
pub const CURVED_EXTRUSION_MIN_SLOPE_DEG: f64 = 0.05;

/// Slope of a segment, in degrees above horizontal. A pure Z hop is 0: it has
/// no horizontal run, so it is not a curved extrusion.
pub fn segment_slope_deg(dx: f64, dy: f64, dz: f64) -> f64 {
    let horizontal = (dx * dx + dy * dy).sqrt();
    if horizontal < 1.0e-6 {
        return 0.0;
    }
    dz.abs().atan2(horizontal).to_degrees()
}

/// How many extruding moves in `gcode` exceed [`CURVED_EXTRUSION_MIN_SLOPE_DEG`].
pub fn count_curved_extrusions(gcode: &str) -> usize {
    let mut count = 0usize;
    walk_extrusions(gcode, |slope| {
        if slope > CURVED_EXTRUSION_MIN_SLOPE_DEG {
            count += 1;
        }
    });
    count
}

/// Steepest extruding move, in degrees, when the file has any extrusion.
pub fn worst_extrusion_slope_deg(gcode: &str) -> Option<f64> {
    let mut worst = 0.0f64;
    let mut seen = false;
    walk_extrusions(gcode, |slope| {
        seen = true;
        if slope > worst {
            worst = slope;
        }
    });
    seen.then_some(worst)
}

fn walk_extrusions(gcode: &str, mut visit: impl FnMut(f64)) {
    let mut pos = [0.0f64; 3];
    let mut e = 0.0f64;
    let mut absolute_xyz = true;
    let mut absolute_e = true;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let code = line.split_whitespace().next().unwrap_or("");
        match code {
            "G90" => absolute_xyz = true,
            "G91" => absolute_xyz = false,
            "M82" => absolute_e = true,
            "M83" => absolute_e = false,
            "G92" => {
                let snapshot = pos;
                for word in line.split_whitespace().skip(1) {
                    let mut chars = word.chars();
                    let axis = chars.next().unwrap_or(' ');
                    let Ok(value) = chars.as_str().parse::<f64>() else {
                        continue;
                    };
                    let slot = match axis {
                        'X' => 0,
                        'Y' => 1,
                        'Z' => 2,
                        _ => continue,
                    };
                    pos[slot] = if absolute_xyz {
                        value
                    } else {
                        snapshot[slot] + value
                    };
                }
            }
            "G0" | "G1" | "G00" | "G01" => {
                let previous = pos;
                let mut next = pos;
                let mut e_word: Option<f64> = None;
                for word in line.split_whitespace().skip(1) {
                    let mut chars = word.chars();
                    let axis = chars.next().unwrap_or(' ');
                    let Ok(value) = chars.as_str().parse::<f64>() else {
                        continue;
                    };
                    if !value.is_finite() {
                        continue;
                    }
                    match axis {
                        'X' => next[0] = if absolute_xyz { value } else { previous[0] + value },
                        'Y' => next[1] = if absolute_xyz { value } else { previous[1] + value },
                        'Z' => next[2] = if absolute_xyz { value } else { previous[2] + value },
                        'E' => e_word = Some(value),
                        _ => {}
                    }
                }
                let extruding = match e_word {
                    Some(value) if absolute_e => {
                        let delta = value - e;
                        e = value;
                        delta > 1.0e-6
                    }
                    Some(value) => {
                        e += value;
                        value > 1.0e-6
                    }
                    None => false,
                };
                if extruding {
                    let slope = segment_slope_deg(
                        next[0] - previous[0],
                        next[1] - previous[1],
                        next[2] - previous[2],
                    );
                    visit(slope);
                }
                pos = next;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_extrusion_is_not_curved_and_a_gentle_rise_is() {
        let flat = "G1 X0 Y0 Z1 E0\nG1 X10 Y0 Z1 E1\n";
        assert_eq!(count_curved_extrusions(flat), 0);
        // atan(0.1/10) is about 0.57°, above the 0.05° curved threshold.
        let rise = "G1 X0 Y0 Z1 E0\nG1 X10 Y0 Z1.1 E1\n";
        assert_eq!(count_curved_extrusions(rise), 1);
        let slope = segment_slope_deg(10.0, 0.0, 0.1);
        assert!((slope - 0.5729).abs() < 0.01, "{slope}");
        let worst = worst_extrusion_slope_deg(rise).unwrap();
        assert!((worst - slope).abs() < 1e-6);
    }

    #[test]
    fn a_z_hop_without_xy_does_not_count() {
        let hop = "G1 X0 Y0 Z1 E0\nG1 Z3 E0.1\n";
        assert_eq!(count_curved_extrusions(hop), 0);
        assert_eq!(segment_slope_deg(0.0, 0.0, 2.0), 0.0);
    }
}
