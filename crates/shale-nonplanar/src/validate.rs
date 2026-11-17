//! Machine-readable G-code safety report after non-planar post-pass.

use crate::collision::{move_collides, PrintedHeightMap};
use crate::config::{CollisionAdjustment, PlanarEquivalenceResult, SafetyReport, SafetyViolation};
use crate::deform::DeformField;
use crate::envelope::HeadEnvelope;
use crate::safety::{gcode_respects_max_angle, max_curved_band_extrusion_slope_deg};

const EXTRUSION_JUMP_MM: f64 = 1.5;
const NONPLANAR_FEED_CAP_MULT: f64 = 0.85;

#[derive(Clone, Debug, Default)]
pub struct MachineLimits {
    pub z_max_mm: f64,
    pub max_feed_mm_per_min: f64,
}

fn word_f(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Read a safety report embedded in G-code by [`embed_safety_json`].
pub fn safety_report_from_gcode(gcode: &str) -> Option<SafetyReport> {
    gcode.lines().find_map(|line| {
        line.trim()
            .strip_prefix(";SHALE_NONPLANAR_SAFETY ")
            .and_then(|payload| serde_json::from_str(payload).ok())
    })
}

pub fn embed_safety_json(gcode: String, report: &SafetyReport) -> String {
    let json = serde_json::to_string(report).unwrap_or_else(|_| "{}".into());
    let marker = format!(";SHALE_NONPLANAR_SAFETY {json}\n");
    let mut out = String::new();
    for line in gcode.lines() {
        if line.trim().starts_with(";SHALE_NONPLANAR_SAFETY ") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if let Some(idx) = out.find(";SHALE_NONPLANAR mode=") {
        let nl = out[idx..].find('\n').map(|o| idx + o + 1).unwrap_or(out.len());
        out.insert_str(nl, &marker);
    } else {
        out = format!("{marker}{out}");
    }
    out
}

pub fn validate_gcode(
    gcode: &str,
    field: &DeformField,
    envelope: &HeadEnvelope,
    limits: &MachineLimits,
    planar_equiv: PlanarEquivalenceResult,
    collision_adjustments: &[CollisionAdjustment],
) -> SafetyReport {
    let band_floor = field.global_band_floor_z();
    let mut max_z = 0.0f64;
    let mut extrusion_jumps = 0usize;
    let mut feed_violations = 0usize;
    let mut violations = Vec::new();
    let mut position = [0.0f64; 4];
    let mut prev_e = 0.0f64;
    let mut have_e = false;

    let slope_cap = field.max_angle_deg + 0.5;
    let max_slope = max_curved_band_extrusion_slope_deg(gcode, field);
    let slope_ok = max_slope.is_none_or(|d| d <= slope_cap);

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
            max_z = max_z.max(z);
        }
        if line.starts_with("G1") && line.contains('E') {
            if let Some(e) = word_f(line, 'E') {
                if have_e {
                    let jump = (e - prev_e).abs();
                    if jump > EXTRUSION_JUMP_MM && position[2] > band_floor + 1e-6 {
                        extrusion_jumps += 1;
                        violations.push(SafetyViolation {
                            kind: "extrusion_jump".into(),
                            detail: format!("ΔE={jump:.3} mm at Z={:.3}", position[2]),
                            line: Some(line.to_string()),
                        });
                    }
                }
                prev_e = e;
                have_e = true;
            }
            if position[2] > band_floor + 1e-6 {
                if let Some(f) = word_f(line, 'F') {
                    let cap = limits.max_feed_mm_per_min * NONPLANAR_FEED_CAP_MULT;
                    if f > cap + 1e-3 {
                        feed_violations += 1;
                        violations.push(SafetyViolation {
                            kind: "feed_rate".into(),
                            detail: format!("F={f:.0} exceeds non-planar cap {cap:.0} mm/min"),
                            line: Some(line.to_string()),
                        });
                    }
                }
            }
        }
        if line.starts_with("G0") {
            have_e = false;
        }
    }

    let z_ok = max_z <= limits.z_max_mm + 1e-3;

    let mut printed = PrintedHeightMap::new([0.0, 0.0], 2.0, 130, 130);
    let mut travel_collisions = 0usize;
    let mut pos = [0.0; 3];
    for raw in gcode.lines() {
        let line = raw.trim();
        if !(line.starts_with('G') && (line.contains('X') || line.contains('Y'))) {
            continue;
        }
        if let Some(x) = word_f(line, 'X') {
            pos[0] = x;
        }
        if let Some(y) = word_f(line, 'Y') {
            pos[1] = y;
        }
        if let Some(z) = word_f(line, 'Z') {
            pos[2] = z;
        }
        let extruding = line.starts_with("G1")
            && line.contains('E')
            && word_f(line, 'E').unwrap_or(0.0) > 1e-9;
        if envelope.calibrated
            && move_collides(
                envelope,
                &printed,
                pos[0],
                pos[1],
                pos[2],
                field.max_angle_deg,
                extruding,
            )
        {
            travel_collisions += 1;
            violations.push(SafetyViolation {
                kind: "travel_collision".into(),
                detail: format!("travel at X={:.2} Y={:.2} Z={:.2}", pos[0], pos[1], pos[2]),
                line: Some(line.to_string()),
            });
        }
        if extruding {
            printed.raise(pos[0], pos[1], pos[2]);
        }
    }

    if !z_ok {
        violations.push(SafetyViolation {
            kind: "z_limit".into(),
            detail: format!("max Z {max_z:.3} > limit {:.3}", limits.z_max_mm),
            line: None,
        });
    }
    if !slope_ok {
        violations.push(SafetyViolation {
            kind: "slope".into(),
            detail: format!(
                "max extrusion slope {:.2}° > cap {:.1}°",
                max_slope.unwrap_or(999.0),
                field.max_angle_deg
            ),
            line: None,
        });
    }
    if !planar_equiv.ok {
        violations.push(SafetyViolation {
            kind: "planar_equivalence".into(),
            detail: planar_equiv
                .first_mismatch
                .clone()
                .unwrap_or_else(|| "below-band mismatch".into()),
            line: None,
        });
    }

    let ok = violations.is_empty()
        && travel_collisions == 0
        && gcode_respects_max_angle(gcode, field)
        && planar_equiv.ok
        && z_ok
        && slope_ok;

    SafetyReport {
        ok,
        printer_id: envelope.printer_id.clone(),
        max_angle_deg: field.max_angle_deg,
        band_floor_z: band_floor,
        z_limit_mm: limits.z_max_mm,
        max_z_seen: max_z,
        z_within_limits: z_ok,
        max_extrusion_slope_deg: max_slope,
        slope_within_cap: slope_ok,
        extrusion_jump_count: extrusion_jumps,
        feed_cap_violations: feed_violations,
        travel_collisions_remaining: travel_collisions,
        envelope_checks_enabled: envelope.calibrated,
        planar_equivalence: planar_equiv,
        collision_adjustments: collision_adjustments.to_vec(),
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deform::{build_deform_field, DeformField};
    use crate::envelope::bambu_a1_envelope;
    use crate::height_field::build_height_field;
    use crate::test_meshes::box_mesh;

    fn sample_field() -> DeformField {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 10.0, 2.0);
        build_deform_field(grid, 2.0, 0.4, 0.2, 10.0)
    }

    #[test]
    fn embed_and_parse_round_trip() {
        let field = sample_field();
        let env = bambu_a1_envelope();
        let limits = MachineLimits {
            z_max_mm: 256.0,
            max_feed_mm_per_min: 6000.0,
        };
        let equiv = PlanarEquivalenceResult {
            ok: true,
            band_floor_z: field.global_band_floor_z(),
            ..Default::default()
        };
        let report = validate_gcode(
            "G1 X0 Y0 Z5 E1 F1800\n",
            &field,
            env,
            &limits,
            equiv,
            &[],
        );
        let gcode = embed_safety_json(String::new(), &report);
        let parsed = safety_report_from_gcode(&gcode).expect("parse");
        assert_eq!(parsed.printer_id, report.printer_id);
    }
}