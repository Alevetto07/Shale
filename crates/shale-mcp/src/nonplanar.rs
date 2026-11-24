// SPDX-License-Identifier: AGPL-3.0-or-later
//! Non-planar options on the MCP surface: input checks and the structured
//! result every tool returns.
//!
//! `shale_core::SettingsPatch` accepts `nonplanar_mode`, `nonplanar_max_angle_deg`
//! and `nonplanar_strict`, but it maps an unknown mode to `off` and forwards any
//! angle, which hides a typo until the part is printed flat. The tools here check
//! the values first and refuse the ones that cannot be honoured, naming the fix.
//!
//! The job's options are mirrored in [`NonplanarSettings`] because `shale_core`
//! exposes no getter for them; `get_settings` reports the mirror. Every slice
//! also returns [`report_digest`], a flat view of
//! [`NonplanarReport`](shale_core::NonplanarReport) with the numbers an agent
//! needs to decide whether to print or to change the job.

use serde_json::{json, Value};
use shale_core::{
    AutoPlacementReport, NonplanarMode, NonplanarOptions, NonplanarReport, PlannedSurface,
    SafetyReport,
};

/// Clearance cap the shipped toolhead envelopes are calibrated for, in degrees.
///
/// Every JSON envelope carries `default_max_clearance_angle_deg: 10`; this is
/// the value the printer data and the collision tolerance were measured for, so
/// a larger angle is refused rather than silently running outside the envelope.
pub const MAX_ANGLE_LIMIT_DEG: f64 = 10.0;

/// Violations quoted in a digest. A job that falls back can carry thousands of
/// collision hits; the first few name the pattern and the counts carry the rest.
const VIOLATIONS_QUOTED: usize = 12;

/// The non-planar settings a job carries, as the MCP client last set them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NonplanarSettings {
    pub mode: NonplanarMode,
    pub max_angle_deg: f64,
    pub strict: bool,
}

impl Default for NonplanarSettings {
    fn default() -> Self {
        let defaults = NonplanarOptions::default();
        Self {
            mode: defaults.mode,
            max_angle_deg: defaults.max_angle_deg,
            strict: defaults.strict,
        }
    }
}

impl NonplanarSettings {
    /// Options for the pipeline calls that need the whole struct.
    pub fn options(self) -> NonplanarOptions {
        NonplanarOptions {
            mode: self.mode,
            max_angle_deg: self.max_angle_deg,
            strict: self.strict,
            ..NonplanarOptions::default()
        }
    }

    /// `get_settings` view: the job's own choices plus what the printer allows.
    pub fn to_json(self, printer_id: &str) -> Value {
        json!({
            "printer_id": printer_id,
            "mode": self.mode.name(),
            "max_angle_deg": self.max_angle_deg,
            "strict": self.strict,
            "supported_by_printer": NonplanarOptions::allowed_for_printer(printer_id),
        })
    }
}

/// `off`, `curved-tops` or `auto` (the aliases the CLI accepts also parse).
///
/// Returns the accepted spellings so a model that guessed wrong can retry
/// without another round of tool discovery. An empty string is refused: the CLI
/// reads it as `off`, but on a tool call it is a client-side mistake.
pub fn parse_mode(mode: &str) -> Result<NonplanarMode, String> {
    if mode.trim().is_empty() {
        return Err("nonplanar_mode is empty; expected off, curved-tops, or auto".into());
    }
    NonplanarMode::from_cli_name(mode).ok_or_else(|| {
        format!("unknown nonplanar_mode {mode:?}; expected off, curved-tops, or auto")
    })
}

/// Reject an angle the pipeline cannot honour.
///
/// Above [`MAX_ANGLE_LIMIT_DEG`] the toolhead envelope is no longer what the
/// printer data was measured for, and zero or less would make every surface
/// steeper than the cap, collapsing the job to a planar slice by design.
pub fn check_max_angle(angle_deg: f64) -> Result<(), String> {
    if !angle_deg.is_finite() || angle_deg <= 0.0 || angle_deg > MAX_ANGLE_LIMIT_DEG {
        return Err(format!(
            "nonplanar_max_angle_deg must be greater than 0 and at most \
             {MAX_ANGLE_LIMIT_DEG:.0} degrees; the shipped toolhead envelopes are \
             calibrated for a {MAX_ANGLE_LIMIT_DEG:.0} degree clearance cap"
        ));
    }
    Ok(())
}

/// Refuse a non-planar mode on a printer with no toolhead envelope.
///
/// Thin wrapper over [`NonplanarOptions::check_printer`] that adds the way out:
/// planar slicing on the same printer is always allowed.
pub fn check_printer(options: NonplanarSettings, printer_id: &str) -> Result<(), String> {
    options
        .options()
        .check_printer(printer_id)
        .map_err(|err| format!("{err}; set nonplanar_mode to off or choose one of those printers"))
}

/// Structured slice result: everything a caller needs to judge a non-planar job.
///
/// Violations are quoted up to [`VIOLATIONS_QUOTED`] with per-kind counts, so a
/// job with thousands of collision hits still answers in a few kilobytes.
pub fn report_digest(report: &NonplanarReport) -> Value {
    json!({
        "mode": report.mode,
        "max_angle_deg": report.max_angle_deg,
        "fallback_to_planar": report.fallback_to_planar,
        "fallback_reasons": report.fallback_reasons,
        "envelope_id": report.envelope_id,
        "top_islands": report.top_islands,
        "islands_flattened": report.islands_flattened,
        "planar_equivalence_ok": report.planar_equivalence_ok,
        "collision_adjustments": report.collision_adjustments.len(),
        "slope_limiting": report.slope_limiting.as_ref().map(|limiting| json!({
            "cap_deg": limiting.cap_deg,
            "passes": limiting.passes,
            "converged": limiting.converged,
            "worst_before_deg": limiting.worst_before_deg,
            "worst_after_deg": limiting.worst_after_deg,
            "cells_reduced": limiting.cells_reduced,
            "min_strength": limiting.min_strength,
        })),
        "safety": report.safety.as_ref().map(safety_digest),
        "auto_plan": report.auto_placement.as_ref().map(placement_digest),
        "notes": report.notes,
    })
}

fn safety_digest(safety: &SafetyReport) -> Value {
    let mut by_kind: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for violation in &safety.violations {
        *by_kind.entry(violation.kind.as_str()).or_default() += 1;
    }
    let quoted = safety
        .violations
        .iter()
        .take(VIOLATIONS_QUOTED)
        .map(|violation| {
            json!({
                "kind": violation.kind,
                "detail": violation.detail,
                "line": violation.line,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "ok": safety.ok,
        "printer_id": safety.printer_id,
        "max_angle_deg": safety.max_angle_deg,
        "slope_cap_deg": safety.slope_cap_deg,
        "max_extrusion_slope_deg": safety.max_extrusion_slope_deg,
        "slope_within_cap": safety.slope_within_cap,
        "z_within_limits": safety.z_within_limits,
        "max_z_seen": safety.max_z_seen,
        "z_limit_mm": safety.z_limit_mm,
        "band_floor_z": safety.band_floor_z,
        "collisions_extruding": safety.collisions_extruding,
        "collisions_travel": safety.collisions_travel,
        "travel_collisions_remaining": safety.travel_collisions_remaining,
        "envelope_checks_enabled": safety.envelope_checks_enabled,
        "extrusion_jump_count": safety.extrusion_jump_count,
        "feed_cap_violations": safety.feed_cap_violations,
        "planar_equivalence": {
            "ok": safety.planar_equivalence.ok,
            "band_floor_z": safety.planar_equivalence.band_floor_z,
            "planar_moves_below_band": safety.planar_equivalence.planar_moves_below_band,
            "curved_moves_below_band": safety.planar_equivalence.curved_moves_below_band,
            "mismatched_moves": safety.planar_equivalence.mismatched_moves,
            "first_mismatch": safety.planar_equivalence.first_mismatch,
        },
        "violation_count": safety.violations.len(),
        "violations_by_kind": by_kind,
        "violations": quoted,
        "violations_truncated": safety.violations.len() > VIOLATIONS_QUOTED,
    })
}

/// Enabled surfaces with their kind, tilt and measured angles; disabled ones
/// with the planner's reason, so a caller can see what auto skipped and why.
fn placement_digest(plan: &AutoPlacementReport) -> Value {
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();
    for surface in &plan.surfaces {
        let entry = surface_digest(surface);
        if surface.enabled {
            enabled.push(entry);
        } else {
            disabled.push(entry);
        }
    }
    json!({
        "mode": plan.mode,
        "max_angle_deg": plan.max_angle_deg,
        "enabled_surface_count": plan.enabled_surface_count,
        "fallback_to_planar": plan.fallback_to_planar,
        "enabled": enabled,
        "disabled": disabled,
        "notes": plan.notes,
    })
}

fn surface_digest(surface: &PlannedSurface) -> Value {
    json!({
        "id": surface.id,
        "feature_id": surface.feature_id,
        "kind": serde_json::to_value(surface.kind).unwrap_or(Value::Null),
        "enabled": surface.enabled,
        "tilt_deg": surface.tilt_deg,
        "bearing_deg": surface.bearing_deg,
        "extent_mm": surface.extent_mm,
        "bounds_min": surface.min,
        "bounds_max": surface.max,
        "overhang_angle_before_deg": surface.overhang_angle_before_deg,
        "overhang_angle_after_deg": surface.overhang_angle_after_deg,
        "overhang_angle_measured_after_deg": surface.overhang_angle_measured_after_deg,
        "slope_measured_from_horizontal_deg": surface.overhang_slope_measured_from_horizontal_deg,
        "reason": surface.reason,
    })
}

/// `plan_nonplanar` result: the detected features, the chosen surfaces, and the
/// reason for each refusal. Preview triangles are a UI overlay, so only their
/// count is returned instead of thousands of floats.
pub fn plan_digest(plan: &AutoPlacementReport) -> Value {
    let mut digest = placement_digest(plan);
    let features = plan
        .detection
        .features
        .iter()
        .map(|feature| {
            json!({
                "id": feature.id,
                "kind": serde_json::to_value(feature.kind).unwrap_or(Value::Null),
                "min": feature.min,
                "max": feature.max,
                "area_mm2": feature.area_mm2,
                "z_center_mm": feature.z_center_mm,
                "max_slope_deg": feature.max_slope_deg,
                "height_spread_mm": feature.height_spread_mm,
                "angle_from_vertical_deg": feature.angle_from_vertical_deg,
                "anchor_ends": feature.anchor_ends,
                "note": feature.note,
            })
        })
        .collect::<Vec<_>>();
    if let Some(object) = digest.as_object_mut() {
        object.insert("detected_features".into(), Value::Array(features));
        object.insert(
            "preview_triangles".into(),
            json!(plan.preview.positions.len() / 9),
        );
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_from_json(value: Value) -> NonplanarReport {
        serde_json::from_value(value).expect("nonplanar report fixture")
    }

    #[test]
    fn mode_spellings_are_the_cli_ones() {
        assert_eq!(parse_mode("off").unwrap(), NonplanarMode::Off);
        assert_eq!(
            parse_mode("curved-tops").unwrap(),
            NonplanarMode::CurvedTops
        );
        assert_eq!(parse_mode("AUTO").unwrap(), NonplanarMode::Auto);
        assert_eq!(
            parse_mode("curved_tops").unwrap(),
            NonplanarMode::CurvedTops
        );
        let err = parse_mode("curvy").expect_err("typo");
        assert!(err.contains("unknown nonplanar_mode \"curvy\""), "{err}");
        assert!(err.contains("curved-tops"), "{err}");
        let empty = parse_mode("  ").expect_err("blank");
        assert!(empty.contains("is empty"), "{empty}");
    }

    #[test]
    fn max_angle_is_inside_the_calibrated_cap() {
        for good in [0.5, 6.0, 10.0] {
            check_max_angle(good).unwrap_or_else(|err| panic!("{good}: {err}"));
        }
        for bad in [0.0, -1.0, 10.5, 45.0, f64::NAN, f64::INFINITY] {
            let err = check_max_angle(bad).expect_err("out of range");
            assert!(
                err.contains("greater than 0 and at most 10"),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn unsupported_printer_is_refused_and_off_is_always_fine() {
        let auto = NonplanarSettings {
            mode: NonplanarMode::Auto,
            ..NonplanarSettings::default()
        };
        check_printer(auto, "bambu_lab_a1").expect("A1 has an envelope");
        check_printer(auto, "bambu_lab_x1_carbon").expect("X1C has an envelope");
        let err = check_printer(auto, "demo_ender").expect_err("no envelope");
        assert!(err.contains("toolhead envelope"), "{err}");
        assert!(err.contains("demo_ender"), "{err}");
        assert!(err.contains("bambu_lab_a1"), "{err}");
        let planar = NonplanarSettings {
            mode: NonplanarMode::Off,
            ..NonplanarSettings::default()
        };
        check_printer(planar, "demo_ender").expect("planar slicing is always allowed");
    }

    #[test]
    fn settings_json_reports_the_printer_budget() {
        let settings = NonplanarSettings {
            mode: NonplanarMode::CurvedTops,
            max_angle_deg: 7.5,
            strict: true,
        };
        let json = settings.to_json("bambu_lab_p1s");
        assert_eq!(json["mode"], "curved-tops");
        assert_eq!(json["max_angle_deg"], 7.5);
        assert_eq!(json["strict"], true);
        assert_eq!(json["supported_by_printer"], true);
        assert_eq!(
            settings.to_json("demo_ender")["supported_by_printer"],
            false
        );
    }

    #[test]
    fn digest_names_the_fallback_and_its_reasons() {
        let report = report_from_json(json!({
            "mode": "auto",
            "max_angle_deg": 10.0,
            "top_islands": 3,
            "islands_flattened": 0,
            "fallback_to_planar": true,
            "notes": ["fallback: 2 travel move(s) collide with printed material"],
            "fallback_reasons": ["2 travel move(s) collide with printed material (tolerance 0.1 mm)"],
            "envelope_id": "bambu_lab_a1",
            "planar_equivalence_ok": true,
            "auto_placement": {
                "mode": "auto",
                "max_angle_deg": 10.0,
                "detection": { "features": [] },
                "surfaces": [
                    {
                        "id": 1,
                        "feature_id": 2,
                        "kind": "curved_top_band",
                        "enabled": true,
                        "min": [0.0, 0.0, 18.0],
                        "max": [40.0, 30.0, 20.0],
                        "tilt_deg": 6.2,
                        "bearing_deg": 90.0,
                        "extent_mm": [40.0, 30.0],
                        "overhang_angle_before_deg": null,
                        "overhang_angle_after_deg": null,
                        "reason": "curved top: curved-top band tilt 6.2 deg (spread 2.00 mm)"
                    },
                    {
                        "id": 2,
                        "feature_id": 3,
                        "kind": "overhang_tilt_band",
                        "enabled": false,
                        "min": [0.0, 0.0, 12.0],
                        "max": [30.0, 20.0, 12.0],
                        "tilt_deg": 10.0,
                        "bearing_deg": 0.0,
                        "extent_mm": [30.0, 20.0],
                        "overhang_angle_before_deg": 90.0,
                        "overhang_angle_after_deg": 80.0,
                        "reason": "horizontal underside 90.0 deg"
                    }
                ],
                "preview": { "positions": [] },
                "enabled_surface_count": 1,
                "fallback_to_planar": false,
                "notes": []
            }
        }));
        let digest = report_digest(&report);
        assert_eq!(digest["mode"], "auto");
        assert_eq!(digest["fallback_to_planar"], true);
        assert_eq!(digest["fallback_reasons"].as_array().unwrap().len(), 1);
        assert_eq!(
            digest["auto_plan"]["enabled"].as_array().unwrap().len(),
            1,
            "{digest}"
        );
        assert_eq!(digest["auto_plan"]["enabled"][0]["kind"], "curved_top_band");
        assert_eq!(digest["auto_plan"]["enabled"][0]["tilt_deg"], 6.2);
        assert_eq!(
            digest["auto_plan"]["disabled"][0]["kind"],
            "overhang_tilt_band"
        );
        assert_eq!(
            digest["auto_plan"]["disabled"][0]["reason"],
            "horizontal underside 90.0 deg"
        );
        // No safety report means no fabricated numbers.
        assert!(digest["safety"].is_null(), "{digest}");
    }

    #[test]
    fn digest_carries_the_safety_report() {
        // Built from JSON so the test does not pin the order or the private
        // shape of `SafetyReport`'s fields.
        let safety: SafetyReport = serde_json::from_value(json!({
            "ok": false,
            "printer_id": "bambu_lab_a1",
            "max_angle_deg": 10.0,
            "band_floor_z": 6.0,
            "z_limit_mm": 250.0,
            "max_z_seen": 20.4,
            "z_within_limits": true,
            "max_extrusion_slope_deg": 11.25,
            "slope_cap_deg": 10.5,
            "slope_within_cap": false,
            "extrusion_jump_count": 0,
            "feed_cap_violations": 0,
            "collisions_extruding": 2,
            "collisions_travel": 1,
            "travel_collisions_remaining": 1,
            "envelope_checks_enabled": true,
            "planar_equivalence": {
                "ok": true,
                "band_floor_z": 6.0,
                "planar_moves_below_band": 120,
                "curved_moves_below_band": 120,
                "mismatched_moves": 0
            },
            "collision_adjustments": [],
            "violations": [{
                "kind": "slope",
                "detail": "11.25 deg above a 10.5 deg cap",
                "line": "G1 X10 Y10 Z9.2 E0.01"
            }]
        }))
        .expect("safety fixture");
        let report = NonplanarReport {
            mode: "curved-tops".into(),
            max_angle_deg: 10.0,
            safety: Some(safety),
            ..NonplanarReport::default()
        };
        let digest = report_digest(&report);
        assert_eq!(digest["safety"]["ok"], false);
        assert_eq!(digest["safety"]["max_extrusion_slope_deg"], 11.25);
        assert_eq!(digest["safety"]["collisions_extruding"], 2);
        assert_eq!(digest["safety"]["collisions_travel"], 1);
        assert_eq!(digest["safety"]["violation_count"], 1);
        assert_eq!(digest["safety"]["violations_by_kind"]["slope"], 1);
        assert_eq!(digest["safety"]["violations"][0]["kind"], "slope");
        assert_eq!(
            digest["safety"]["violations"][0]["line"],
            "G1 X10 Y10 Z9.2 E0.01"
        );
        assert_eq!(
            digest["safety"]["planar_equivalence"]["mismatched_moves"],
            0
        );
        assert_eq!(digest["safety"]["planar_equivalence"]["ok"], true);
    }
}
