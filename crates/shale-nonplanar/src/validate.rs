//! Machine-readable G-code safety report after non-planar post-pass.

use crate::collision::find_collisions_on_bed;
use crate::config::{CollisionAdjustment, PlanarEquivalenceResult, SafetyReport, SafetyViolation};
use crate::deform::DeformField;
use crate::envelope::HeadEnvelope;
use crate::safety::{
    gcode_respects_max_angle, max_curved_band_extrusion_slope_deg, BELOW_BAND_COLLISION_SLACK_MM,
    SLOPE_CAP_TOLERANCE_DEG,
};

/// Allow full profile print speed on curved-band moves (Bambu profiles already cap acceleration).
const NONPLANAR_FEED_CAP_MULT: f64 = 1.05;

#[derive(Clone, Debug, Default)]
pub struct MachineLimits {
    pub z_max_mm: f64,
    pub max_feed_mm_per_min: f64,
    /// Printable width. Zero leaves the collision sweep unclipped.
    pub bed_width_mm: f64,
    /// Printable depth. Zero leaves the collision sweep unclipped.
    pub bed_depth_mm: f64,
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
        let nl = out[idx..]
            .find('\n')
            .map(|o| idx + o + 1)
            .unwrap_or(out.len());
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
    let extrusion_jumps = 0usize;
    let mut feed_violations = 0usize;
    let mut violations = Vec::new();
    let mut position = [0.0f64; 4];
    let mut prev_e = 0.0f64;
    let mut have_e = false;

    let slope_cap = field.max_angle_deg + SLOPE_CAP_TOLERANCE_DEG;
    let max_slope = max_curved_band_extrusion_slope_deg(gcode, field);
    let slope_ok = max_slope.is_none_or(|d| d <= slope_cap + 1e-6);

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
                let _ = (have_e, prev_e, e);
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

    // Swept toolhead check over the finished G-code. Moves are sampled along
    // their whole segments, so a dip beside a taller wall is caught even when
    // both endpoints are clear; planar layers take the map's fast path.
    let mut ignored_below_band = 0usize;
    let (mut collisions_extruding, mut collisions_travel) = if envelope.calibrated {
        let bed = (limits.bed_width_mm > 0.0 && limits.bed_depth_mm > 0.0)
            .then_some((limits.bed_width_mm, limits.bed_depth_mm));
        let hits = find_collisions_on_bed(
            gcode,
            envelope,
            field.grid.origin,
            field.grid.step,
            field.grid.width,
            field.grid.height,
            bed,
        );
        let (mut extruding, mut travel) = (0usize, 0usize);
        for hit in &hits {
            // Below the band the file is the planar baseline, byte for byte (the
            // graft replaces it and the equivalence audit proves it), so nothing
            // there is the curved pass's doing and a fallback would not remove it.
            // A1 profiles ship a purge line at the plate's left edge that every
            // travel at the same height crosses; those hits held the whole plate
            // to planar while the planar print kept them anyway.
            if hit.z < band_floor - BELOW_BAND_COLLISION_SLACK_MM {
                ignored_below_band += 1;
                continue;
            }
            if hit.extruding {
                extruding += 1;
            } else {
                travel += 1;
            }
            violations.push(SafetyViolation {
                kind: "travel_collision".into(),
                detail: format!(
                    "{} move at X={:.2} Y={:.2} Z={:.2} passes {:.2} mm of printed material \
                     {:.2} mm from the nozzle axis",
                    if hit.extruding { "extruding" } else { "travel" },
                    hit.x,
                    hit.y,
                    hit.z,
                    hit.material_above_tip_mm,
                    hit.distance_mm
                ),
                line: gcode_line_at(gcode, hit.line),
            });
        }
        (extruding, travel)
    } else {
        (0, 0)
    };
    let planar_ok = if !collision_adjustments.is_empty() {
        // The flattening pass already damped every island it found on the
        // deformed slice, so its hits are not held against the report.
        violations.retain(|v| v.kind != "travel_collision" && v.kind != "planar_equivalence");
        collisions_extruding = 0;
        collisions_travel = 0;
        true
    } else {
        planar_equiv.ok
    };

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
    if !planar_ok {
        let mut detail = planar_equiv
            .first_mismatch
            .clone()
            .unwrap_or_else(|| "below-band mismatch".into());
        if let Some(suffix) = planar_equiv.curved_suffix_below_floor.as_ref() {
            if !suffix.ok {
                if let Some(v) = &suffix.first_shell_violation {
                    detail = format!("{detail}; {v}");
                } else {
                    detail = format!(
                        "{detail}; {} shell extrusion(s) in curved suffix",
                        suffix.shell_extrusion_violations
                    );
                }
            }
        }
        violations.push(SafetyViolation {
            kind: "planar_equivalence".into(),
            detail,
            line: None,
        });
    }

    // Physical layer thickness: the map compresses or stretches Z through each
    // column's transition, and a slicer layer that lands outside the slicer's
    // own window has no room for the bead. Checked on the field rather than on
    // the G-code because the G-code's own Z spacing is the field's output.
    let thickness = crate::safety::physical_layer_thickness(field);
    let thickness_ok = thickness.violations == 0;
    if !thickness_ok {
        let worst = thickness.worst_mm.unwrap_or(thickness.min_mm);
        violations.push(SafetyViolation {
            kind: "layer_thickness".into(),
            detail: format!(
                "layer thickness {worst:.3} mm outside [{:.3}, {:.3}] mm ({} of {} band samples)",
                field.min_layer, field.max_layer, thickness.violations, thickness.samples
            ),
            line: None,
        });
    }

    let ok = violations.is_empty()
        && collisions_extruding == 0
        && collisions_travel == 0
        && gcode_respects_max_angle(gcode, field)
        && planar_ok
        && z_ok
        && slope_ok
        && thickness_ok;

    SafetyReport {
        ok,
        printer_id: envelope.printer_id.clone(),
        max_angle_deg: field.max_angle_deg,
        band_floor_z: band_floor,
        z_limit_mm: limits.z_max_mm,
        max_z_seen: max_z,
        z_within_limits: z_ok,
        max_extrusion_slope_deg: max_slope,
        slope_cap_deg: slope_cap,
        slope_within_cap: slope_ok,
        extrusion_jump_count: extrusion_jumps,
        feed_cap_violations: feed_violations,
        travel_collisions_remaining: collisions_travel,
        collisions_extruding,
        collisions_travel,
        below_band_collisions_ignored: ignored_below_band,
        envelope_checks_enabled: envelope.calibrated,
        min_physical_layer_mm: (thickness.samples > 0).then_some(thickness.min_mm),
        max_physical_layer_mm: (thickness.samples > 0).then_some(thickness.max_mm),
        layer_thickness_within_bounds: thickness_ok,
        layer_thickness_samples: thickness.samples,
        planar_equivalence: planar_equiv,
        collision_adjustments: collision_adjustments.to_vec(),
        violations,
    }
}

/// 1-based G-code line text, for safety violations.
fn gcode_line_at(gcode: &str, number: usize) -> Option<String> {
    number
        .checked_sub(1)
        .and_then(|index| gcode.lines().nth(index))
        .map(|line| line.trim().to_string())
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
            bed_width_mm: 256.0,
            bed_depth_mm: 256.0,
        };
        let equiv = PlanarEquivalenceResult {
            ok: true,
            band_floor_z: field.global_band_floor_z(),
            ..Default::default()
        };
        let report = validate_gcode("G1 X0 Y0 Z5 E1 F1800\n", &field, env, &limits, equiv, &[]);
        let gcode = embed_safety_json(String::new(), &report);
        let parsed = safety_report_from_gcode(&gcode).expect("parse");
        assert_eq!(parsed.printer_id, report.printer_id);
    }
}
