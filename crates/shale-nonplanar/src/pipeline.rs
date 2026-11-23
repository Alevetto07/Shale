//! Orchestration: one field, built once, used both ways.
//!
//! The field is built from the merged MACHINE-space reference mesh before any
//! model is deformed, slope-limited before slicing, and then reused verbatim by
//! the post-pass for the inverse map. The post-pass only verifies: it never
//! rebuilds the field, re-plans placement, or edits the field to hide a failure.
//! When a check fails the job falls back to the planar baseline (or fails, in
//! strict mode) instead of silently bending geometry.

use crate::auto::{apply_plan_to_field, plan_auto_placement, PlannedSurfaceKind, SupportSpanInput};
use crate::collision::PrintedHeightMap;
use crate::config::{NonplanarMode, NonplanarOptions, NonplanarReport};
use crate::deform::{build_deform_field, deform_mesh, subdivide_long_edges, DeformField};
use crate::envelope::{head_envelope_for_printer, missing_envelope_message};
use crate::equivalence::{check_grafted_planar_equivalence, graft_planar_shell_below_band};
use crate::height_field::{build_height_field, count_top_islands};
use crate::inverse::map_gcode_inverse;
use crate::mesh::{merge_meshes as merge_meshes_internal, Mesh};
use crate::slope_limit::{limit_field_slopes, SlopeLimitSummary};
use crate::validate::{embed_safety_json, validate_gcode, MachineLimits};

/// Everything the post-pass needs, produced once by [`prepare_plate`].
#[derive(Clone, Debug)]
pub struct NonplanarContext {
    /// The whole deformation: height grid, auto plan and pre-slice slope safety.
    pub field: DeformField,
    pub options: NonplanarOptions,
    pub printer_id: String,
    pub nominal_layer_height: f64,
    pub top_islands: usize,
    pub slope_limit: SlopeLimitSummary,
    pub auto_placement: Option<crate::auto::AutoPlacementReport>,
    /// Set when the pre-pass refused to deform, so the job must print planar.
    pub preslice_failure: Option<String>,
    pub notes: Vec<String>,
}

impl NonplanarContext {
    /// Longest straight edge the deformation can follow without folding the mesh.
    pub fn max_edge_mm(&self) -> f64 {
        self.field.grid.step * 3.0
    }
}

/// Build the plate's single deformation field.
///
/// The reference mesh must be the merged, MACHINE-space mesh of every model on
/// the plate, so the auto plan, the spans and the height grid all describe the
/// same geometry the models are deformed with and the G-code is inverse mapped
/// from.
pub fn prepare_plate(
    reference_mesh: &Mesh,
    options: &NonplanarOptions,
    nominal_layer_height: f64,
    printer_id: &str,
    support_spans: &[SupportSpanInput],
) -> Result<NonplanarContext, String> {
    if head_envelope_for_printer(printer_id).is_none() {
        return Err(missing_envelope_message(printer_id));
    }
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
    let mut notes = Vec::new();
    let mut auto_placement = None;
    if options.mode == NonplanarMode::Auto {
        let plan =
            plan_auto_placement(reference_mesh, support_spans, options, nominal_layer_height);
        apply_plan_to_field(&mut field, &plan, nominal_layer_height);
        notes.extend(plan.notes.iter().cloned());
        if plan.fallback_to_planar {
            notes.push("auto placement enabled no surface; the top stays flat".into());
        }
        auto_placement = Some(plan);
    }
    // Slope safety converges here, before CuraEngine slices the deformed mesh,
    // so the geometry that gets sliced and the field used to invert it agree.
    let slope_limit = limit_field_slopes(&mut field);
    notes.push(slope_limit.note());
    let preslice_failure = if slope_limit.converged {
        None
    } else {
        Some(format!(
            "slope limiting did not converge: worst {:.2} deg over a {:.2} deg cap with {} fixed band conflict(s)",
            slope_limit.worst_after_deg, slope_limit.max_angle_deg, slope_limit.fixed_conflicts
        ))
    };
    Ok(NonplanarContext {
        top_islands: count_top_islands(&field.grid),
        field,
        options: options.clone(),
        printer_id: printer_id.to_ascii_lowercase(),
        nominal_layer_height,
        slope_limit,
        auto_placement,
        preslice_failure,
        notes,
    })
}

/// Deform one model in MACHINE space with the plate field.
///
/// The caller applies [`crate::mesh::to_engine_space`] afterwards; that is a
/// pure XY shift, so the field, which is in machine space, stays valid.
pub fn deform_model(mesh: &Mesh, ctx: &NonplanarContext) -> Mesh {
    if ctx.preslice_failure.is_some() {
        return mesh.clone();
    }
    let subdivided = subdivide_long_edges(mesh, ctx.max_edge_mm());
    deform_mesh(&subdivided, &ctx.field)
}

pub fn merge_meshes(meshes: &[Mesh]) -> Mesh {
    merge_meshes_internal(meshes)
}

/// Inverse map the curved slice, graft the planar shell, verify, and either
/// accept the result or fall back to the planar baseline.
///
/// # Errors
/// Only in strict mode, or when the printer has no head envelope (which
/// [`prepare_plate`] already refuses).
pub fn apply_post_pass(
    deformed_slice_gcode: &str,
    planar_baseline_gcode: &str,
    ctx: &NonplanarContext,
    limits: MachineLimits,
) -> Result<(String, NonplanarReport), String> {
    let envelope = head_envelope_for_printer(&ctx.printer_id)
        .ok_or_else(|| missing_envelope_message(&ctx.printer_id))?;
    let mut report = NonplanarReport::default();
    report.mode = ctx.options.mode.name().into();
    report.max_angle_deg = ctx.field.max_angle_deg;
    report.top_islands = ctx.top_islands;
    report.notes.extend(ctx.notes.iter().cloned());
    if let Some(plan) = &ctx.auto_placement {
        report.fallback_to_planar |= plan.fallback_to_planar;
        report.auto_placement = Some(plan.clone());
    }
    if let Some(reason) = &ctx.preslice_failure {
        return fail_clean(ctx, report, planar_baseline_gcode, vec![reason.clone()]);
    }

    report
        .notes
        .push("ironing disabled for curved-top pass".into());
    let mapped = map_gcode_inverse(&ctx.field, deformed_slice_gcode, ctx.nominal_layer_height);
    if let Some(plan) = &mut report.auto_placement {
        fill_measured_overhang_angles(&mapped, plan);
    }
    let band_floor = ctx.field.global_band_floor_z();
    let grafted = graft_planar_shell_below_band(planar_baseline_gcode, &mapped, band_floor);
    let equiv = check_grafted_planar_equivalence(planar_baseline_gcode, &grafted, &ctx.field);
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
    let safety = validate_gcode(&grafted, &ctx.field, envelope, &limits, equiv);
    report.notes.push(format!(
        "slope verified: {} (worst {:.2} deg against a {:.2} deg cap)",
        ctx.slope_limit.note(),
        ctx.slope_limit.worst_after_deg,
        ctx.slope_limit.max_angle_deg
    ));
    if !safety.ok {
        report
            .notes
            .push(format!("safety report: {} violation(s)", safety.violations.len()));
    }

    // Every failed check sends the job to the planar baseline. Nothing is
    // repaired by editing geometry or the field after slicing.
    let mut reasons: Vec<String> = Vec::new();
    if !crate::safety::gcode_respects_max_angle(&grafted, &ctx.field) {
        let worst = crate::safety::max_curved_band_extrusion_slope_deg(&grafted, &ctx.field)
            .unwrap_or(f64::NAN);
        reasons.push(format!(
            "slope: curved band reaches {worst:.2} deg over a {:.2} deg cap",
            ctx.field.max_angle_deg
        ));
    }
    for violation in &safety.violations {
        reasons.push(format!("{}: {}", violation.kind, violation.detail));
    }
    if !report.planar_equivalence_ok {
        reasons.push("planar equivalence: below-band G-code differs from the planar slice".into());
    }
    if !reasons.is_empty() {
        report.safety = Some(safety);
        return fail_clean(ctx, report, planar_baseline_gcode, reasons);
    }
    report.safety = Some(safety);

    let header = format_nonplanar_header(&report);
    let (mut out, report) = ok_attach_header(grafted, &header, report);
    if let Some(safety) = report.safety.as_ref() {
        out = embed_safety_json(out, safety);
    }
    Ok((out, report))
}

/// Fall back to the planar baseline (or fail in strict mode), with the exact
/// reasons in the notes and in the `;SHALE_NONPLANAR` header.
fn fail_clean(
    ctx: &NonplanarContext,
    mut report: NonplanarReport,
    planar_baseline: &str,
    reasons: Vec<String>,
) -> Result<(String, NonplanarReport), String> {
    if ctx.options.strict {
        return Err(format!(
            "non-planar verification failed: {}",
            reasons.join("; ")
        ));
    }
    report.fallback_to_planar = true;
    report.fallback_reasons = reasons.clone();
    for reason in &reasons {
        report.notes.push(format!("fell back to planar: {reason}"));
    }
    let header = format_nonplanar_header(&report);
    let (mut out, report) = ok_attach_header(planar_baseline.to_string(), &header, report);
    if let Some(safety) = report.safety.as_ref() {
        out = embed_safety_json(out, safety);
    }
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
        let slope = max_underside_band_slope_deg(
            gcode,
            surface.min,
            surface.max,
            surface.bearing_deg,
            8,
        )
        .or_else(|| max_extrusion_slope_in_box(gcode, surface.min, surface.max));
        if let Some(slope) = slope.filter(|&s| s > 0.25) {
            surface.overhang_slope_measured_from_horizontal_deg = Some(slope);
            surface.overhang_angle_measured_after_deg = Some((90.0 - slope).clamp(0.0, 90.0));
        }
    }
}

fn format_nonplanar_header(report: &NonplanarReport) -> String {
    let mut header = format!(
        ";SHALE_NONPLANAR mode={} max_angle={:.1} islands={} flattened={} fallback={} equiv={} safe={}",
        report.mode,
        report.max_angle_deg,
        report.top_islands,
        report.islands_flattened,
        report.fallback_to_planar,
        report.planar_equivalence_ok,
        report.safety.as_ref().is_some_and(|s| s.ok)
    );
    if !report.fallback_reasons.is_empty() {
        let reason = report
            .fallback_reasons
            .join("; ")
            .replace(['"', '\n', '\r'], " ");
        header.push_str(&format!(" reason=\"{reason}\""));
    }
    header.push('\n');
    header
}

fn ok_attach_header(mapped: String, header: &str, mut report: NonplanarReport) -> (String, NonplanarReport) {
    let mut out = header.to_string();
    out.push_str(&mapped);
    report.notes.push("post-pass complete".into());
    (out, report)
}

/// Envelope self-test on finished G-code: must find no collision on any move.
///
/// Returns `false` for a printer without an envelope, so an unsupported machine
/// can never be reported as safe.
pub fn gcode_respects_envelope(gcode: &str, printer_id: &str) -> bool {
    let Some(envelope) = head_envelope_for_printer(printer_id) else {
        return false;
    };
    let step = 1.0;
    let mut printed = PrintedHeightMap::new([0.0, 0.0], step, 260, 260);
    let mut position = [0.0; 3];
    let mut prev = [0.0; 3];
    let mut have_prev = false;
    for raw in gcode.lines() {
        let line = raw.trim();
        let motion = (line.starts_with("G0") || line.starts_with("G1"))
            && (line.contains('X') || line.contains('Y') || line.contains('Z'));
        if let Some(x) = parse_word(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = parse_word(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = parse_word(line, 'Z') {
            position[2] = z;
        }
        if !motion {
            continue;
        }
        let e_pos = parse_word(line, 'E').unwrap_or(0.0);
        let extruding = line.starts_with("G1") && line.contains('E') && e_pos > 0.0;
        if !have_prev {
            prev = position;
            have_prev = true;
            continue;
        }
        if crate::collision::segment_collides(
            envelope,
            &printed,
            prev,
            position,
            step,
            crate::collision::CONTACT_TOL_MM,
        )
        .is_some()
        {
            return false;
        }
        if extruding {
            printed.raise_segment([prev[0], prev[1]], [position[0], position[1]], position[2], step);
        }
        prev = position;
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
                if horiz > VERTICAL_STEP_HORIZ_MM {
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
            if horiz > VERTICAL_STEP_HORIZ_MM {
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
                if horiz > VERTICAL_STEP_HORIZ_MM {
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
    use crate::test_meshes::box_mesh;

    #[test]
    fn post_pass_equivalence_and_safety_on_box() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let opts = NonplanarOptions {
            mode: NonplanarMode::CurvedTops,
            max_angle_deg: 10.0,
            grid_step_mm: 1.0,
            ..NonplanarOptions::default()
        };
        let planar = "G1 X5 Y5 Z2 E0\nG1 X15 Y5 Z2 E1\nG0 X15 Y15 Z9\nG1 X15 Y15 Z9 E2\nG1 X5 Y15 Z9.2 E3\n";
        let ctx = prepare_plate(&mesh, &opts, 0.2, "bambu_lab_a1", &[]).expect("context");
        let (out, report) = apply_post_pass(
            planar,
            planar,
            &ctx,
            MachineLimits {
                z_max_mm: 256.0,
                max_feed_mm_per_min: 6000.0,
            },
        )
        .expect("post pass");
        assert!(report.planar_equivalence_ok, "{:?}", report.notes);
        assert!(report.safety.as_ref().is_some_and(|s| s.ok), "{out}");
        assert!(out.contains(";SHALE_NONPLANAR_SAFETY"));
    }

    /// The G-code handed to the post-pass is sliced at *deformed* Z. Mapping it
    /// back with the same field must return the physical top surface, which is
    /// the whole point of building the field once.
    #[test]
    fn inverse_map_round_trips_the_top_surface() {
        let mesh = crate::test_meshes::tilted_top_slab(40.0, 40.0, 10.0, 6.0);
        let opts = NonplanarOptions {
            mode: NonplanarMode::CurvedTops,
            max_angle_deg: 10.0,
            grid_step_mm: 0.5,
            ..NonplanarOptions::default()
        };
        let ctx = prepare_plate(&mesh, &opts, 0.2, "bambu_lab_a1", &[]).expect("context");
        assert!(ctx.preslice_failure.is_none(), "{:?}", ctx.preslice_failure);
        assert!(
            ctx.field.deformed_top_z > 0.0,
            "the field must bend the top, not leave it alone"
        );

        // Synthetic planar G-code: one move per column, written at the Z the
        // deformed mesh has there.
        // A travel move first, exactly as a slicer emits: the inverse map needs
        // a known position before the first extrusion.
        let mut gcode = String::from("G90\n");
        if let Some(top) = ctx.field.grid.sample_top_smooth(6.0, 6.0) {
            let z_def = ctx.field.forward_z(6.0, 6.0, top);
            gcode.push_str(&format!("G0 X6.000 Y6.000 Z{z_def:.4}\n"));
        }
        let mut expected: std::collections::HashMap<(String, String), f64> =
            std::collections::HashMap::new();
        let mut x = -18.0;
        while x <= 18.0 {
            let mut y = -18.0;
            while y <= 18.0 {
                if let Some(top) = ctx.field.grid.sample_top_smooth(x, y) {
                    let z_def = ctx.field.forward_z(x, y, top);
                    gcode.push_str(&format!("G1 X{x:.3} Y{y:.3} Z{z_def:.4} E1\n"));
                    expected.insert((format!("{x:.3}"), format!("{y:.3}")), top);
                }
                y += 1.0;
            }
            x += 1.0;
        }

        let mapped = map_gcode_inverse(&ctx.field, &gcode, ctx.nominal_layer_height);
        let mut checked = 0usize;
        let mut worst = 0.0f64;
        for line in mapped.lines() {
            let (Some(px), Some(py), Some(pz)) = (
                word_after(line, 'X'),
                word_after(line, 'Y'),
                word_after(line, 'Z'),
            ) else {
                continue;
            };
            let key = (format!("{px:.3}"), format!("{py:.3}"));
            let Some(physical) = expected.get(&key) else {
                continue;
            };
            checked += 1;
            worst = worst.max((pz - physical).abs());
        }
        assert!(checked > 200, "only {checked} points were inverse mapped");
        assert!(
            worst <= 0.02,
            "inverse map misses the physical top by {worst:.4} mm over {checked} points"
        );

        // The sampled top is the mesh top: the field describes the geometry the
        // engine sliced, not a smoothed stand-in.
        let analytic = |x: f64| 10.0 + x * 6.0f64.to_radians().tan();
        let mut worst_tie = 0.0f64;
        let mut cell_x = -16.0;
        while cell_x <= 16.0 {
            let mut cell_y = -16.0;
            while cell_y <= 16.0 {
                if let Some(top) = ctx.field.grid.sample_top_smooth(cell_x, cell_y) {
                    worst_tie = worst_tie.max((top - analytic(cell_x)).abs());
                }
                cell_y += 2.0;
            }
            cell_x += 2.0;
        }
        assert!(
            worst_tie <= 0.02,
            "sampled top drifts {worst_tie:.4} mm from the mesh"
        );
    }

    fn word_after(line: &str, letter: char) -> Option<f64> {
        let start = line.find(letter)? + 1;
        let rest = &line[start..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .unwrap_or(rest.len());
        rest[..end].parse().ok()
    }
}