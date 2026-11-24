//! Orchestration: one shared field for the pre-pass and the post-pass.

use serde::{Deserialize, Serialize};

use crate::auto::{apply_plan_to_field, plan_auto_placement, PlannedSurfaceKind, SupportSpanInput};
use crate::collision::PrintedHeightMap;
use crate::config::{NonplanarMode, NonplanarOptions, NonplanarReport, SlopeLimitingReport};
use crate::deform::{build_deform_field, deform_mesh, subdivide_long_edges, DeformField};
use crate::envelope::{require_head_envelope, COLLISION_TOLERANCE_MM};
use crate::equivalence::{check_grafted_planar_equivalence, graft_planar_shell_below_band};
use crate::height_field::build_height_field;
use crate::inverse::{map_gcode_inverse_counted, MAX_INVERSE_CLAMP_MM};
use crate::mesh::{merge_meshes as merge_meshes_internal, Mesh};
use crate::safety::{
    limit_field_slopes, restrict_to_printable_top, PLANAR_COLLAPSE_STRENGTH,
    SLOPE_CAP_TOLERANCE_DEG,
};
use crate::validate::{embed_safety_json, validate_gcode, MachineLimits};

/// Everything the post-pass needs from the pre-pass.
///
/// The field is built once, from the merged machine-space reference mesh of every
/// model in the job, and carried here whole. The post-pass maps the sliced G-code
/// with exactly the field that shaped the meshes, so the inverse is the inverse of
/// the forward map instead of the inverse of a second, differently-planned field.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NonplanarContext {
    pub field: DeformField,
    pub nominal_layer_height: f64,
    pub max_angle_deg: f64,
    /// Connected top sheets the field flattens.
    pub top_islands: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_placement: Option<crate::auto::AutoPlacementReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope_limiting: Option<SlopeLimitingReport>,
    /// Set when no printable deformation is left inside the slope cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planar_collapsed: Option<String>,
}

impl NonplanarContext {
    /// Number of top sheets that still receive a lift above their own surface.
    ///
    /// Gated on [`DeformField::max_strength`], not the minimum: in auto the
    /// footprint is a subset of the top, so the minimum is 0 on every honest field.
    pub fn islands_flattened(&self) -> usize {
        if self.planar_collapsed.is_some() || self.field.max_strength() <= PLANAR_COLLAPSE_STRENGTH
        {
            0
        } else {
            self.top_islands
        }
    }
}

/// Build the single field every model in the job is deformed with.
///
/// `reference_mesh` must be the merged **machine-space** mesh of all models: the
/// height field is sampled in the same coordinates as the G-code the post-pass
/// rewrites, and one field for the whole plate keeps the auto spans, the auto
/// placement and the slope limiting consistent between the two passes.
pub fn plan_field(
    reference_mesh: &Mesh,
    options: &NonplanarOptions,
    nominal_layer_height: f64,
    support_spans: &[SupportSpanInput],
) -> NonplanarContext {
    let band = options.effective_band_thickness(nominal_layer_height);
    let ramp = options.effective_ramp_thickness(nominal_layer_height);
    let grid = build_height_field(
        reference_mesh,
        options.grid_step_mm,
        options.max_angle_deg,
        band,
    );
    let mut field = build_deform_field(
        grid,
        band,
        ramp,
        nominal_layer_height,
        options.max_angle_deg,
    );
    let mut auto_placement = None;
    if options.mode == NonplanarMode::Auto {
        let plan =
            plan_auto_placement(reference_mesh, support_spans, options, nominal_layer_height);
        apply_plan_to_field(&mut field, &plan, nominal_layer_height);
        // The plan tilts the top sheet, so the flat targets have to be read again.
        field.refresh_top_targets();
        auto_placement = Some(plan);
    }
    // The band follows `z_top_smooth`, so the footprint is what the cap has to be
    // checked against — not the lift schedule. Cut every column whose own surface
    // is steeper than the cap (a smoothed rim, a shoulder, the flank of a dome) out
    // of the footprint and let the planar slicer keep it; the cut boundary is then
    // ramped at `tan(cap)` so it has no step of its own. Without this the field
    // carries a rim on every flat deck whose only fix is damping the island to 0.
    let footprint_cut = restrict_to_printable_top(&mut field);
    let slope_limiting = Some(limit_field_slopes(&mut field, nominal_layer_height));
    let mut planar_collapsed = None;
    if auto_placement
        .as_ref()
        .is_some_and(|plan| plan.fallback_to_planar)
    {
        planar_collapsed =
            Some("auto placement found no surface it could enable on this geometry".to_string());
    }
    if let Some(limiting) = &slope_limiting {
        // max, not min: the footprint is a subset of the top, so every column the
        // plan did not cover and every column the cap cut sits at exactly 0. The
        // only question is whether anything is still lifting.
        if limiting.max_strength <= PLANAR_COLLAPSE_STRENGTH {
            planar_collapsed = Some(format!(
                "slope limiting left {:.1}% strength on the best column after {} pass(es) \
                 (worst level surface {:.2}° -> {:.2}°, cap {:.1}°, {} of {} top column(s) cut \
                 by the cap): nothing is left to print curved",
                limiting.max_strength * 100.0,
                limiting.passes,
                limiting.worst_before_deg,
                limiting.worst_after_deg,
                limiting.cap_deg,
                footprint_cut,
                field.engaged_columns() + footprint_cut
            ));
        }
    }
    NonplanarContext {
        nominal_layer_height,
        max_angle_deg: options.max_angle_deg,
        top_islands: field.top_islands,
        auto_placement,
        slope_limiting,
        planar_collapsed,
        field,
    }
}

/// Deform one model with the shared field. `mesh` is in machine space.
pub fn deform_with_field(mesh: &Mesh, field: &DeformField) -> Mesh {
    let subdivided = subdivide_long_edges(mesh, 1.0);
    deform_mesh(&subdivided, field)
}

/// Pre-pass for a single mesh: plan a field from it and deform it.
///
/// Jobs with more than one model call [`plan_field`] on the merged plate instead,
/// so every model shares one field; this stays for single-model callers and tests.
pub fn prepare_pre_pass(
    mesh: &Mesh,
    options: &NonplanarOptions,
    nominal_layer_height: f64,
    support_spans: &[SupportSpanInput],
) -> (Mesh, NonplanarContext) {
    let ctx = plan_field(mesh, options, nominal_layer_height, support_spans);
    let deformed = deform_with_field(mesh, &ctx.field);
    (deformed, ctx)
}

pub fn merge_meshes(meshes: &[Mesh]) -> Mesh {
    merge_meshes_internal(meshes)
}

/// Rebuild an in-memory field from mesh + options.
///
/// Only for callers that never ran a pre-pass (fixtures, unit tests). The
/// post-pass must use the field carried in [`NonplanarContext`]: rebuilding it
/// from a different mesh is what made the inverse inexact before M1.
pub fn rebuild_field(
    mesh: &Mesh,
    options: &NonplanarOptions,
    nominal_layer_height: f64,
) -> DeformField {
    plan_field(mesh, options, nominal_layer_height, &[]).field
}

/// Arguments of the post-pass: the sliced G-code, the planar baseline from the
/// same job, and the context the deformed meshes were sliced with.
pub struct PostPassArgs<'a> {
    pub deformed_slice_gcode: &'a str,
    pub planar_baseline_gcode: &'a str,
    pub context: &'a NonplanarContext,
    pub options: &'a NonplanarOptions,
    pub printer_id: &'a str,
    pub limits: MachineLimits,
}

/// Post-pass on deformed-mesh G-code from CuraEngine.
///
/// `planar_baseline_gcode` must be a **planar** slice of the same job on the
/// undeformed meshes (physical machine coordinates): it is both the below-band
/// graft source and the file the job falls back to when the curved output cannot
/// be verified.
///
/// Returns `Err` only in [`NonplanarOptions::strict`] mode; otherwise an
/// unverifiable pass returns the planar baseline with the reasons recorded.
pub fn apply_post_pass(args: PostPassArgs<'_>) -> Result<(String, NonplanarReport), String> {
    let PostPassArgs {
        deformed_slice_gcode,
        planar_baseline_gcode,
        context,
        options,
        printer_id,
        limits,
    } = args;
    let field = &context.field;
    let mut report = NonplanarReport {
        mode: options.mode.name().into(),
        max_angle_deg: options.max_angle_deg,
        top_islands: context.top_islands,
        islands_flattened: context.islands_flattened(),
        slope_limiting: context.slope_limiting.clone(),
        ..NonplanarReport::default()
    };
    // The head envelope is measured data, not a default: an unknown printer has
    // no verified clearance, so it cannot be cleared for a curved pass.
    let envelope = match require_head_envelope(printer_id) {
        Ok(envelope) => envelope,
        Err(err) => {
            return planar_fallback(
                report,
                vec![err.to_string()],
                planar_baseline_gcode,
                options,
            )
        }
    };
    report
        .notes
        .push("ironing disabled for curved-top pass".into());
    if let Some(plan) = context.auto_placement.clone() {
        report.fallback_to_planar |= plan.fallback_to_planar;
        report.auto_placement = Some(plan);
    }
    // Nothing to map: the pre-pass already decided this geometry cannot be printed
    // inside the slope cap, so hand back the planar baseline the job sliced.
    if let Some(reason) = context.planar_collapsed.clone() {
        return planar_fallback(report, vec![reason], planar_baseline_gcode, options);
    }
    let (mapped, stats) =
        map_gcode_inverse_counted(field, deformed_slice_gcode, context.nominal_layer_height);
    report.inverse_corrections = Some(stats.clone());
    if let Some(plan) = &mut report.auto_placement {
        fill_measured_overhang_angles(&mapped, plan);
    }
    let band_floor = field.global_band_floor_z();
    let grafted = graft_planar_shell_below_band(planar_baseline_gcode, &mapped, band_floor);
    let equiv = check_grafted_planar_equivalence(planar_baseline_gcode, &grafted, field);
    report.planar_equivalence_ok = equiv.ok;
    if !equiv.ok {
        report.notes.push(format!(
            "planar equivalence: {} mismatches below z={:.3}",
            equiv.mismatched_moves, equiv.band_floor_z
        ));
    } else {
        report
            .notes
            .push("planar equivalence: below-band G-code matches planar slice".into());
    }
    if let Some(suffix) = equiv.curved_suffix_below_floor.as_ref() {
        if suffix.ok {
            report.notes.push(format!(
                "curved suffix below z={:.3}: {} travel, {} band-edge extrusion, 0 shell extrusion",
                suffix.band_floor_z, suffix.travel, suffix.band_edge_extrusion
            ));
        } else {
            report.notes.push(format!(
                "curved suffix: {} planar-shell extrusion(s) below tolerance",
                suffix.shell_extrusion_violations
            ));
        }
    }
    let safety = validate_gcode(
        &grafted,
        field,
        envelope,
        &limits,
        equiv.clone(),
        &report.collision_adjustments,
    );
    // Collision verification: the head and gantry must not sweep through printed
    // material. `validate_gcode` samples every move against the printed height
    // map with the calibrated head envelope; a hit means the curved file is not
    // printable, so the job goes back to its planar baseline. The field is never
    // edited to route around a collision: that changes the printed shape.
    let mut reasons: Vec<String> = Vec::new();
    if !safety.envelope_checks_enabled {
        reasons.push(format!(
            "no calibrated head envelope for {printer_id}, collision sweep skipped"
        ));
    }
    if safety.collisions_extruding > 0 {
        reasons.push(format!(
            "{} extruding move(s) collide with printed material (tolerance {:.1} mm)",
            safety.collisions_extruding, COLLISION_TOLERANCE_MM
        ));
    }
    if safety.collisions_travel > 0 {
        reasons.push(format!(
            "{} travel move(s) collide with printed material (tolerance {:.1} mm)",
            safety.collisions_travel, COLLISION_TOLERANCE_MM
        ));
    }
    if let Some(limiting) = &report.slope_limiting {
        if !limiting.converged
            || limiting.worst_after_deg > limiting.cap_deg + SLOPE_CAP_TOLERANCE_DEG
        {
            reasons.push(format!(
                "slope limiting left a {:.2}° level surface (cap {:.1}°, {} passes)",
                limiting.worst_after_deg, limiting.cap_deg, limiting.passes
            ));
        }
    }
    if !safety.z_within_limits {
        reasons.push(format!(
            "Z {:.2} mm outside machine range (limit {:.2} mm)",
            safety.max_z_seen, safety.z_limit_mm
        ));
    }
    if !safety.slope_within_cap {
        reasons.push(format!(
            "toolhead would follow {:.2}° > {:.2}° (cap {:.1}° + {:.1}° tolerance)",
            safety.max_extrusion_slope_deg.unwrap_or(f64::NAN),
            safety.slope_cap_deg,
            safety.max_angle_deg,
            SLOPE_CAP_TOLERANCE_DEG
        ));
    }
    if !equiv.ok {
        reasons.push(format!(
            "planar equivalence: {} mismatched move(s) below the band",
            equiv.mismatched_moves
        ));
    }
    if stats.unclamped_excess > 0 {
        reasons.push(format!(
            "{} inverse-map move(s) needed more than the {:.2} mm slope-clamp budget",
            stats.unclamped_excess, MAX_INVERSE_CLAMP_MM
        ));
    }
    // Only a collision the curved pass *introduces* counts. The machine's own
    // start/end script is in both files: on the X1 Carbon the final
    // `G0 X65 Y10 Z0` parks the head through the 0.15 mm prime line it laid at the
    // start of the same print, so the raw check rejected every X1 file for a move
    // the deformation never touched (0 collisions reported by the swept sweep).
    // If the planar baseline fails the same check, the script is at fault and the
    // curved pass is not.
    if !gcode_respects_envelope(&grafted, printer_id, options.max_angle_deg)
        && gcode_respects_envelope(planar_baseline_gcode, printer_id, options.max_angle_deg)
    {
        reasons.push("a curved move leaves the head envelope".to_string());
    }
    if !safety.ok && safety.violations.is_empty() {
        reasons.push("safety report failed".to_string());
    }
    // One or two concrete samples help debugging; the counts above carry the
    // magnitude, and a thousand identical lines make the report unreadable.
    const SAMPLES: usize = 3;
    for violation in safety.violations.iter().take(SAMPLES) {
        reasons.push(format!("{}: {}", violation.kind, violation.detail));
    }
    if safety.violations.len() > SAMPLES {
        reasons.push(format!(
            "... and {} more violation(s), see the safety report",
            safety.violations.len() - SAMPLES
        ));
    }
    report.notes.push(format!(
        "safety report: {} violation(s)",
        safety.violations.len()
    ));
    report.safety = Some(safety);
    report.inverse_corrections = Some(stats.clone());
    if !reasons.is_empty() {
        return planar_fallback(report, reasons, planar_baseline_gcode, options);
    }
    let header = format_nonplanar_header(&report);
    let (mut out, report) = ok_attach_header(grafted, &header, report);
    if let Some(safety) = report.safety.as_ref() {
        out = embed_safety_json(out, safety);
    }
    Ok((out, report))
}

/// Emit the planar baseline instead of a curved file that cannot be verified.
///
/// Strict mode turns the same reasons into an error: a caller that asked for a
/// guarantee must not be handed a flat print it did not ask for.
fn planar_fallback(
    mut report: NonplanarReport,
    reasons: Vec<String>,
    planar_baseline_gcode: &str,
    options: &NonplanarOptions,
) -> Result<(String, NonplanarReport), String> {
    if options.strict {
        return Err(format!(
            "nonplanar: curved pass rejected: {}",
            reasons.join("; ")
        ));
    }
    report.fallback_to_planar = true;
    report.islands_flattened = 0;
    report.planar_equivalence_ok = true;
    for reason in &reasons {
        report.notes.push(format!("fallback: {reason}"));
        report.fallback_reasons.push(reason.clone());
    }
    let header = format_nonplanar_header(&report);
    let mut out = header;
    out.push_str(planar_baseline_gcode);
    Ok((out, report))
}

fn fill_measured_overhang_angles(gcode: &str, plan: &mut crate::auto::AutoPlacementReport) {
    for surface in &mut plan.surfaces {
        if !surface.enabled {
            continue;
        }
        let underside = matches!(
            surface.kind,
            PlannedSurfaceKind::BridgeTiltBand
                | PlannedSurfaceKind::CantileverTiltBand
                | PlannedSurfaceKind::OverhangTiltBand
        );
        if !underside {
            continue;
        }
        let slope =
            max_underside_band_slope_deg(gcode, surface.min, surface.max, surface.bearing_deg, 8)
                .or_else(|| max_extrusion_slope_in_box(gcode, surface.min, surface.max));
        if let Some(slope) = slope.filter(|&s| s > 0.25) {
            surface.overhang_slope_measured_from_horizontal_deg = Some(slope);
            surface.overhang_angle_measured_after_deg = Some((90.0 - slope).clamp(0.0, 90.0));
        }
    }
}

fn format_nonplanar_header(report: &NonplanarReport) -> String {
    let mut line = format!(
        ";SHALE_NONPLANAR mode={} max_angle={:.1} islands={} flattened={} fallback={} equiv={} safe={}",
        report.mode,
        report.max_angle_deg,
        report.top_islands,
        report.islands_flattened,
        report.fallback_to_planar,
        report.planar_equivalence_ok,
        report.safety.as_ref().is_some_and(|s| s.ok)
    );
    if let Some(limiting) = &report.slope_limiting {
        line.push_str(&format!(
            " slope_limited={} passes={} worst={:.2}->{:.2} cap={:.1}",
            limiting.converged,
            limiting.passes,
            limiting.worst_before_deg,
            limiting.worst_after_deg,
            limiting.cap_deg
        ));
    }
    if !report.fallback_reasons.is_empty() {
        line.push_str(&format!(
            " fallback_reason=\"{}\"",
            report.fallback_reasons.join(" | ")
        ));
    }
    line.push('\n');
    line
}

fn ok_attach_header(
    mapped: String,
    header: &str,
    mut report: NonplanarReport,
) -> (String, NonplanarReport) {
    let mut out = header.to_string();
    out.push_str(&mapped);
    report.notes.push("post-pass complete".into());
    (out, report)
}

/// Build a printed height map envelope self-test on finished G-code (no extrusion moves should violate).
pub fn gcode_respects_envelope(gcode: &str, printer_id: &str, max_angle_deg: f64) -> bool {
    // Fail closed: an unknown printer has no measured clearance, so it does not
    // get the A1 numbers by default.
    let Ok(envelope) = require_head_envelope(printer_id) else {
        return false;
    };
    let step = 2.0;
    // Machine-coordinate G-code from Cura (front-left origin on a 256 mm bed).
    let mut printed = PrintedHeightMap::new([0.0, 0.0], step, 130, 130);
    let mut position = [0.0; 3];
    for raw in gcode.lines() {
        let line = raw.trim();
        if !(line.starts_with('G') && (line.contains('X') || line.contains('Y'))) {
            continue;
        }
        if let Some(x) = parse_word(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = parse_word(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = parse_word(line, 'Z') {
            position[2] = z;
        }
        let e_pos = parse_word(line, 'E').unwrap_or(0.0);
        let extruding = line.starts_with("G1") && line.contains('E') && e_pos > 0.0;
        if line.starts_with('G')
            && (extruding || line.starts_with("G0"))
            && crate::collision::move_collides(
                envelope,
                &printed,
                position[0],
                position[1],
                position[2],
                max_angle_deg,
                extruding,
            )
        {
            return false;
        }
        if extruding {
            printed.raise(position[0], position[1], position[2]);
        }
    }
    true
}

fn parse_word(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// `None` marks a travel break (G0) before the next extrusion point.
type ShellExtrusionChain = Vec<Option<[f64; 3]>>;

/// Same-XY layer hops are not nozzle tilt in XY (see curved-band slope safety).
const VERTICAL_STEP_HORIZ_MM: f64 = 0.05;

/// Shortest horizontal run a measured bead slope may span.
///
/// Below one line width the ratio `dz/horiz` is a polygon vertex, not a direction:
/// a pole ring on a sphere, the last segment before a contour closes, or the
/// quarter-layer step where a row of beads passes from the mapped shell to the
/// planar one. The toolhead never travels that way for longer than a bead is wide,
/// and the swept head check (0.1 mm clearance) owns clearance there. Counting them
/// read 25.8 deg at the 24 mm ball's pole while every real bead sat at 7.6.
const BEAD_SLOPE_MIN_RUN_MM: f64 = 0.4;

fn max_slope_on_moves(moves: &[ShellExtrusionChain], across_layers: bool) -> f64 {
    let mut max_deg = 0.0f64;
    let mut prev: Option<[f64; 3]> = None;
    for layer in moves {
        if !across_layers {
            prev = None;
        }
        for point in layer {
            let Some(point) = *point else {
                prev = None;
                continue;
            };
            if let Some(p) = prev {
                let dx = point[0] - p[0];
                let dy = point[1] - p[1];
                let dz = point[2] - p[2];
                let horiz = (dx * dx + dy * dy).sqrt();
                if horiz > 2.5 {
                    prev = Some(point);
                    continue;
                }
                if horiz > BEAD_SLOPE_MIN_RUN_MM {
                    let deg = dz.atan2(horiz).abs().to_degrees();
                    max_deg = max_deg.max(deg);
                }
            }
            prev = Some(point);
        }
    }
    max_deg
}

/// Max in-layer consecutive extrusion slope (deg from horizontal) in a bottom band, along `bearing_deg`.
/// Travel moves (G0) and leaving the box break the extrusion chain (no sorted point windows).
pub fn max_underside_band_slope_deg(
    gcode: &str,
    min: [f64; 3],
    max: [f64; 3],
    bearing_deg: f64,
    layer_shells: usize,
) -> Option<f64> {
    let (ux, uy) = {
        let r = bearing_deg.to_radians();
        (r.cos(), r.sin())
    };
    let layer_h = ((max[2] - min[2]) / layer_shells as f64).max(0.08);
    let max_layer_step = layer_h * 1.35;

    fn in_band(point: [f64; 3], min: [f64; 3], max: [f64; 3]) -> bool {
        point[0] >= min[0] - 0.05
            && point[0] <= max[0] + 0.05
            && point[1] >= min[1] - 0.05
            && point[1] <= max[1] + 0.05
            && point[2] >= min[2] - 0.05
            && point[2] <= max[2] + 0.2
    }

    let mut position = [0.0f64; 3];
    let mut prev_extrusion: Option<[f64; 3]> = None;
    let mut max_deg = 0.0f64;
    let mut any = false;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with("G0") && (line.contains('X') || line.contains('Y')) {
            prev_extrusion = None;
        }
        if let Some(x) = parse_word(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = parse_word(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = parse_word(line, 'Z') {
            position[2] = z;
        }
        let extruding = line.starts_with("G1")
            && line.contains('E')
            && parse_word(line, 'E').unwrap_or(0.0) > 1e-9;
        if !extruding {
            continue;
        }
        let point = position;
        if !in_band(point, min, max) {
            prev_extrusion = None;
            continue;
        }
        if let Some(prev) = prev_extrusion {
            let dx = point[0] - prev[0];
            let dy = point[1] - prev[1];
            let dz = point[2] - prev[2];
            if dz.abs() > max_layer_step {
                prev_extrusion = Some(point);
                continue;
            }
            let along = (dx * ux + dy * uy).abs();
            let cross = (dx * -uy + dy * uy).abs();
            let horiz = (dx * dx + dy * dy).sqrt();
            if along < 0.15 || cross > along || horiz <= VERTICAL_STEP_HORIZ_MM {
                prev_extrusion = Some(point);
                continue;
            }
            let deg = dz.atan2(horiz).abs().to_degrees();
            any = true;
            max_deg = max_deg.max(deg);
        }
        prev_extrusion = Some(point);
    }
    any.then_some(max_deg)
}

/// Maximum in-layer extrusion slope (deg from horizontal) inside an axis-aligned box.
pub fn max_extrusion_slope_in_box(gcode: &str, min: [f64; 3], max: [f64; 3]) -> Option<f64> {
    let mut position = [0.0f64; 3];
    let mut prev_extrusion: Option<[f64; 3]> = None;
    let mut max_deg = 0.0f64;
    let mut any = false;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with("G0") && (line.contains('X') || line.contains('Y')) {
            prev_extrusion = None;
        }
        if let Some(x) = parse_word(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = parse_word(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = parse_word(line, 'Z') {
            position[2] = z;
        }
        let extruding = line.starts_with("G1")
            && line.contains('E')
            && parse_word(line, 'E').unwrap_or(0.0) > 1e-9;
        if !extruding {
            continue;
        }
        let point = position;
        let in_box = point[0] >= min[0] - 0.05
            && point[0] <= max[0] + 0.05
            && point[1] >= min[1] - 0.05
            && point[1] <= max[1] + 0.05
            && point[2] >= min[2] - 0.05
            && point[2] <= max[2] + 0.05;
        if !in_box {
            prev_extrusion = if in_box { Some(point) } else { None };
            continue;
        }
        if let Some(prev) = prev_extrusion {
            let dx = point[0] - prev[0];
            let dy = point[1] - prev[1];
            let dz = point[2] - prev[2];
            let horiz = (dx * dx + dy * dy).sqrt();
            if horiz > BEAD_SLOPE_MIN_RUN_MM {
                any = true;
                max_deg = max_deg.max(dz.atan2(horiz).abs().to_degrees());
            }
        }
        prev_extrusion = Some(point);
    }
    any.then_some(max_deg)
}

pub fn max_extrusion_slope_above_z(gcode: &str, z_min: f64) -> Option<f64> {
    let mut position = [0.0f64; 3];
    let mut prev_extrusion: Option<[f64; 3]> = None;
    let mut max_deg = 0.0f64;
    let mut any = false;
    for raw in gcode.lines() {
        let line = raw.trim();
        let positional = line.starts_with("G0") || line.starts_with("G1");
        if !positional && !line.contains('X') && !line.contains('Y') && !line.contains('Z') {
            continue;
        }
        if let Some(x) = parse_word(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = parse_word(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = parse_word(line, 'Z') {
            position[2] = z;
        }
        let extruding = line.starts_with("G1")
            && line.contains('E')
            && parse_word(line, 'E').unwrap_or(0.0) > 1e-9;
        if !extruding {
            // A slope is only defined along a path. A move that carries the nozzle
            // somewhere else without extruding — a travel, a retract, a layer change
            // — ends the path; measuring across it compares the end of one contour
            // with the start of the next and reads the layer step as a slope. That
            // artefact reported 25.8 deg on the 24 mm ball while every printed bead
            // sat at 7.6.
            prev_extrusion = None;
            continue;
        }
        let point = position;
        if point[2] < z_min - 1e-6 {
            prev_extrusion = Some(point);
            continue;
        }
        if let Some(prev) = prev_extrusion {
            if prev[2] >= z_min - 1e-6 {
                let dx = point[0] - prev[0];
                let dy = point[1] - prev[1];
                let dz = point[2] - prev[2];
                let horiz = (dx * dx + dy * dy).sqrt();
                if horiz > BEAD_SLOPE_MIN_RUN_MM {
                    any = true;
                    max_deg = max_deg.max(dz.atan2(horiz).abs().to_degrees());
                }
            }
        }
        prev_extrusion = Some(point);
    }
    any.then_some(max_deg)
}

/// Maximum slope from the vertical on the top layer's extrusion moves (degrees).
pub fn max_top_layer_slope_deg(gcode: &str) -> Option<f64> {
    let moves = top_layer_extrusion_moves(gcode)?;
    Some(max_slope_on_moves(std::slice::from_ref(&moves), false))
}

/// Maximum slope over the upper ``layer_count`` extruding layers.
pub fn max_top_shell_slope_deg(gcode: &str, layer_count: usize) -> Option<f64> {
    let moves = top_shell_extrusion_moves(gcode, layer_count)?;
    Some(max_slope_on_moves(&moves, false))
}

/// Same as [`max_top_shell_slope_deg`] but keeps slope across layer boundaries (ramps).
pub fn max_top_shell_slope_across_layers_deg(gcode: &str, layer_count: usize) -> Option<f64> {
    let (z_lo, _) = top_shell_z_variation(gcode, layer_count)?;
    max_extrusion_slope_above_z(gcode, z_lo - 1e-6)
}

fn top_shell_extrusion_moves(gcode: &str, layer_count: usize) -> Option<Vec<ShellExtrusionChain>> {
    let mut layers: Vec<ShellExtrusionChain> = Vec::new();
    let mut current: ShellExtrusionChain = Vec::new();
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut travel_break = false;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with(";LAYER:") {
            if !current.is_empty() {
                layers.push(std::mem::take(&mut current));
            }
            travel_break = false;
            continue;
        }
        if line.starts_with("G0") && (line.contains('X') || line.contains('Y')) {
            travel_break = true;
        }
        if let Some(xx) = parse_word(line, 'X') {
            x = xx;
        }
        if let Some(yy) = parse_word(line, 'Y') {
            y = yy;
        }
        if let Some(zz) = parse_word(line, 'Z') {
            z = zz;
        }
        if line.starts_with("G1") && line.contains('E') {
            if let Some(e) = parse_word(line, 'E') {
                if e > 1e-9 {
                    if travel_break {
                        current.push(None);
                        travel_break = false;
                    }
                    current.push(Some([x, y, z]));
                }
            }
        }
    }
    if !current.is_empty() {
        layers.push(current);
    }
    let start = layers.len().saturating_sub(layer_count);
    if start >= layers.len() {
        None
    } else {
        Some(layers[start..].to_vec())
    }
}

fn top_layer_extrusion_moves(gcode: &str) -> Option<ShellExtrusionChain> {
    let moves = top_shell_extrusion_moves(gcode, 1)?;
    moves.into_iter().next()
}

/// Z variation on the upper ``layer_count`` extruding layers (default top shell).
pub fn top_shell_z_variation(gcode: &str, layer_count: usize) -> Option<(f64, f64)> {
    let mut layers: Vec<Vec<f64>> = Vec::new();
    let mut current = Vec::new();
    let mut z = 0.0f64;
    for raw in gcode.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix(";LAYER:") {
            if !current.is_empty() {
                layers.push(std::mem::take(&mut current));
            }
            let _ = rest.parse::<i32>();
            continue;
        }
        if let Some(zz) = parse_word(line, 'Z') {
            z = zz;
        }
        if line.starts_with("G1") && line.contains('E') {
            current.push(z);
        }
    }
    if !current.is_empty() {
        layers.push(current);
    }
    let start = layers.len().saturating_sub(layer_count);
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for layer in &layers[start..] {
        for &zz in layer {
            min_z = min_z.min(zz);
            max_z = max_z.max(zz);
        }
    }
    if min_z.is_finite() && max_z.is_finite() {
        Some((min_z, max_z))
    } else {
        None
    }
}

/// Top-layer Z variation in parsed G-code (max - min Z on last layer block).
pub fn top_layer_z_variation(gcode: &str) -> Option<(f64, f64)> {
    let mut layers: Vec<Vec<f64>> = Vec::new();
    let mut current = Vec::new();
    let mut z = 0.0f64;
    for raw in gcode.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix(";LAYER:") {
            if !current.is_empty() {
                layers.push(std::mem::take(&mut current));
            }
            let _ = rest.parse::<i32>();
            continue;
        }
        if let Some(zz) = parse_word(line, 'Z') {
            z = zz;
        }
        if line.starts_with("G1") && line.contains('E') {
            current.push(z);
        }
    }
    if !current.is_empty() {
        layers.push(current);
    }
    let last = layers.last()?;
    let min_z = last.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_z = last.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    Some((min_z, max_z))
}

/// Histogram of in-layer segment slopes (degrees) over the upper shell extrusion moves.
pub fn in_layer_slope_histogram(
    gcode: &str,
    layer_count: usize,
    bin_width_deg: f64,
) -> Vec<(f64, usize)> {
    let Some(layers) = top_shell_extrusion_moves(gcode, layer_count) else {
        return Vec::new();
    };
    use std::collections::BTreeMap;
    let mut bins: BTreeMap<i32, usize> = BTreeMap::new();
    let width = bin_width_deg.max(0.1);
    for layer in &layers {
        let mut prev: Option<[f64; 3]> = None;
        for point in layer {
            let Some(point) = *point else {
                prev = None;
                continue;
            };
            if let Some(p) = prev {
                let dx = point[0] - p[0];
                let dy = point[1] - p[1];
                let dz = point[2] - p[2];
                let horiz = (dx * dx + dy * dy).sqrt();
                if horiz > 2.5 {
                    prev = Some(point);
                    continue;
                }
                if horiz > 1e-6 {
                    let deg = dz.atan2(horiz).abs().to_degrees();
                    let bin = (deg / width).floor() as i32;
                    *bins.entry(bin).or_default() += 1;
                }
            }
            prev = Some(point);
        }
    }
    bins.into_iter()
        .map(|(b, n)| ((b as f64 + 0.5) * width, n))
        .collect()
}

#[cfg(test)]
mod underside_slope_tests {
    use super::max_underside_band_slope_deg;

    #[test]
    fn consecutive_extrusion_ignores_cross_layer_sort_pairs() {
        let gcode = "\
G1 X0 Y0 Z10.0 E1\n\
G1 X20 Y0 Z10.5 E2\n\
G0 X5 Y0\n\
G1 X0 Y0 Z12.0 E3\n\
G1 X20 Y0 Z12.5 E4\n";
        let min = [-1.0, -1.0, 9.5];
        let max = [21.0, 1.0, 13.0];
        let slope = max_underside_band_slope_deg(gcode, min, max, 0.0, 8).expect("slope");
        assert!(
            slope < 5.0,
            "must not stitch unrelated layers via sorted windows (saw {slope:.2}°)"
        );
    }
}

#[cfg(test)]
mod post_pass_tests {
    use super::*;
    use crate::config::NonplanarMode;
    use crate::safety::max_field_layer_slope_deg;
    use crate::test_meshes::{box_mesh, sphere_mesh};

    fn limits() -> MachineLimits {
        MachineLimits {
            z_max_mm: 256.0,
            max_feed_mm_per_min: 6000.0,
        }
    }

    fn options(mode: NonplanarMode, max_angle_deg: f64) -> NonplanarOptions {
        NonplanarOptions {
            mode,
            max_angle_deg,
            grid_step_mm: 1.0,
            ..NonplanarOptions::default()
        }
    }

    fn translated(mesh: &Mesh, dx: f64) -> Mesh {
        let mut out = mesh.clone();
        for v in &mut out.vertices {
            v[0] += dx;
        }
        out
    }

    fn top_z(mesh: &Mesh, x_min: f64, x_max: f64) -> f64 {
        mesh.vertices
            .iter()
            .filter(|v| v[0] >= x_min && v[0] <= x_max)
            .fold(f64::NEG_INFINITY, |acc, v| acc.max(v[2]))
    }

    /// COORD acceptance B: the inverse map is the inverse of the forward map for
    /// the *same* field, both algebraically and through the G-code text.
    #[test]
    fn shared_field_round_trip_is_exact() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let ctx = plan_field(&mesh, &options(NonplanarMode::CurvedTops, 10.0), 0.2, &[]);
        let field = &ctx.field;
        assert!(
            field.min_strength() > 0.0,
            "a flat-top box must keep a live deformation"
        );
        for (x, y) in [
            (-8.0, -8.0),
            (-4.0, 0.0),
            (0.0, 0.0),
            (4.0, 6.0),
            (8.0, 8.0),
        ] {
            for z_phys in [8.6, 9.2, 9.7] {
                let z_def = field.forward_z(x, y, z_phys);
                let back = field.inverse_z(x, y, z_def);
                assert!(
                    (back - z_phys).abs() < 1e-6,
                    "inverse(forward({z_phys})) = {back} at ({x}, {y})"
                );
            }
        }
        // Same round trip through the G-code the post-pass actually rewrites: the
        // deformed file says Z = forward_z, the map must give the planar Z back.
        let z_phys = 9.6;
        let z_def = field.forward_z(0.0, 0.0, z_phys);
        let deformed = format!("G1 X0 Y0 Z{z_def:.4} E1\nG1 X1 Y0 Z{z_def:.4} E2\n");
        let (mapped, stats) = map_gcode_inverse_counted(field, &deformed, 0.2);
        let zs: Vec<f64> = mapped.lines().filter_map(|l| parse_word(l, 'Z')).collect();
        assert!(!zs.is_empty(), "map produced no Z: {mapped}");
        for z in &zs {
            assert!(
                (z - z_phys).abs() < 1e-3,
                "mapped Z {z} should be the planar {z_phys} (all: {zs:?})"
            );
        }
        assert_eq!(
            stats.unclamped_excess, 0,
            "a legal move needs no clamp budget"
        );
    }

    /// Two models on one plate share the field, and each island keeps its own
    /// flat target instead of being squashed to the tallest model on the plate.
    #[test]
    fn one_field_serves_every_model_with_island_targets() {
        let tall = box_mesh(10.0, 10.0, 12.0);
        let short = translated(&box_mesh(10.0, 10.0, 6.0), 20.0);
        let reference = merge_meshes(&[tall.clone(), short.clone()]);
        let ctx = plan_field(
            &reference,
            &options(NonplanarMode::CurvedTops, 10.0),
            0.2,
            &[],
        );
        assert!(
            ctx.top_islands >= 2,
            "two separated boxes are two top islands, got {}",
            ctx.top_islands
        );
        let tall_out = deform_with_field(&tall, &ctx.field);
        let short_out = deform_with_field(&short, &ctx.field);
        let tall_top = top_z(&tall_out, -5.0, 5.0);
        let short_top = top_z(&short_out, 15.0, 25.0);
        assert!(
            (tall_top - 12.0).abs() < 0.3,
            "tall box top {tall_top} should stay at its own 12 mm"
        );
        assert!(
            (short_top - 6.0).abs() < 0.3,
            "short box top {short_top} must stay at its own 6 mm, not be pulled to 12"
        );
    }

    #[test]
    fn post_pass_equivalence_and_safety_on_box() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let opts = options(NonplanarMode::CurvedTops, 10.0);
        let ctx = plan_field(&mesh, &opts, 0.2, &[]);
        let planar =
            "G1 X-5 Y-5 Z2 E0\nG1 X5 Y-5 Z2 E1\nG0 X5 Y5 Z9\nG1 X5 Y5 Z9 E2\nG1 X-5 Y5 Z9.2 E3\n";
        let (out, report) = apply_post_pass(PostPassArgs {
            deformed_slice_gcode: planar,
            planar_baseline_gcode: planar,
            context: &ctx,
            options: &opts,
            printer_id: "bambu_lab_a1",
            limits: limits(),
        })
        .expect("non-strict post-pass never errors");
        assert!(
            !report.fallback_to_planar,
            "flat-top box must print curved, reasons: {:?}",
            report.fallback_reasons
        );
        assert!(report.planar_equivalence_ok, "{:?}", report.notes);
        assert!(report.safety.as_ref().is_some_and(|s| s.ok), "{out}");
        assert!(out.contains(";SHALE_NONPLANAR"));
        assert!(out.contains(";SHALE_NONPLANAR_SAFETY"));
        let limiting = report.slope_limiting.as_ref().expect("limiting report");
        assert!(limiting.converged, "{limiting:?}");
    }

    /// A dome whose level surfaces cannot fit under the cap must be reported and
    /// fallen back, never Z-clamped into a shape the printer cannot follow.
    #[test]
    fn dome_converges_or_reports_a_clean_fallback() {
        let mesh = sphere_mesh(12.0);
        for cap in [45.0, 20.0, 10.0, 2.0] {
            let opts = options(NonplanarMode::CurvedTops, cap);
            let ctx = plan_field(&mesh, &opts, 0.2, &[]);
            let limiting = ctx.slope_limiting.as_ref().expect("limiting report");
            if let Some(reason) = &ctx.planar_collapsed {
                assert!(
                    limiting.min_strength <= 0.02 || limiting.worst_after_deg > limiting.cap_deg,
                    "collapse at {cap}° must be explained by the limiter: {limiting:?}"
                );
                let planar = "G1 X-4 Y-4 Z2 E0\nG1 X4 Y-4 Z2 E1\nG1 X4 Y4 Z9 E2\n";
                let (out, report) = apply_post_pass(PostPassArgs {
                    deformed_slice_gcode: planar,
                    planar_baseline_gcode: planar,
                    context: &ctx,
                    options: &opts,
                    printer_id: "bambu_lab_a1",
                    limits: limits(),
                })
                .expect("non-strict fallback is Ok");
                assert!(report.fallback_to_planar, "{cap}°: {:?}", report.notes);
                assert!(!report.fallback_reasons.is_empty());
                assert!(out.contains("fallback=true"), "{out}");
                assert!(out.contains("fallback_reason="), "{out}");
                assert!(
                    out.ends_with(planar),
                    "fallback must be the untouched planar baseline"
                );
                let strict = NonplanarOptions {
                    strict: true,
                    ..opts.clone()
                };
                let err = apply_post_pass(PostPassArgs {
                    deformed_slice_gcode: planar,
                    planar_baseline_gcode: planar,
                    context: &ctx,
                    options: &strict,
                    printer_id: "bambu_lab_a1",
                    limits: limits(),
                })
                .expect_err("strict mode must refuse");
                assert!(err.contains("curved pass rejected"), "{err}");
                assert!(
                    !reason.is_empty(),
                    "the collapse reason must say why the job went planar"
                );
            } else {
                assert!(limiting.converged, "{cap}°: {limiting:?}");
                let worst = max_field_layer_slope_deg(&ctx.field);
                assert!(
                    worst <= cap + SLOPE_CAP_TOLERANCE_DEG,
                    "{cap}°: field still holds a {worst:.2}° level surface"
                );
            }
        }
    }

    #[test]
    fn post_pass_header_carries_the_slope_summary() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let opts = options(NonplanarMode::CurvedTops, 10.0);
        let ctx = plan_field(&mesh, &opts, 0.2, &[]);
        let planar = "G1 X-5 Y-5 Z2 E0\nG1 X5 Y-5 Z2 E1\n";
        let (out, _) = apply_post_pass(PostPassArgs {
            deformed_slice_gcode: planar,
            planar_baseline_gcode: planar,
            context: &ctx,
            options: &opts,
            printer_id: "bambu_lab_a1",
            limits: limits(),
        })
        .expect("ok");
        let header = out.lines().next().expect("header");
        assert!(header.starts_with(";SHALE_NONPLANAR "), "{header}");
        assert!(header.contains(" slope_limited="), "{header}");
        assert!(header.contains("passes="), "{header}");
    }
}
