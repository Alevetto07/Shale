//! Orchestration: pre-pass mesh deform and post-pass inverse G-code map.

use serde::{Deserialize, Serialize};
use crate::mesh::{merge_meshes as merge_meshes_internal, Mesh};

use crate::collision::{flatten_on_collision, PrintedHeightMap};
use crate::config::{NonplanarOptions, NonplanarReport};
use crate::deform::{build_deform_field, deform_mesh, subdivide_long_edges, DeformField};
use crate::envelope::{head_envelope_for_printer, bambu_a1_envelope};
use crate::equivalence::{check_planar_equivalence, graft_planar_shell_below_band};
use crate::validate::{embed_safety_json, validate_gcode, MachineLimits};
use crate::height_field::{build_height_field, count_top_islands};
use crate::safety::enforce_slope_safety;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NonplanarContext {
    pub field: DeformFieldSnapshot,
    pub nominal_layer_height: f64,
    pub max_angle_deg: f64,
}

/// Serializable subset for attaching to slice outcomes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeformFieldSnapshot {
    pub band_thickness: f64,
    pub ramp_thickness: f64,
    pub deformed_top_z: f64,
    pub nominal_layer: f64,
    pub top_islands: usize,
}

/// Pre-pass: build height field, deform mesh, return context for post-pass.
pub fn prepare_pre_pass(
    mesh: &Mesh,
    options: &NonplanarOptions,
    nominal_layer_height: f64,
) -> (Mesh, NonplanarContext) {
    let band = options.effective_band_thickness(nominal_layer_height);
    let ramp = options.effective_ramp_thickness(nominal_layer_height);
    let grid = build_height_field(mesh, options.grid_step_mm, options.max_angle_deg, band);
    let field = build_deform_field(grid, band, ramp, nominal_layer_height, options.max_angle_deg);
    let islands = count_top_islands(&field.grid);
    let subdivided = subdivide_long_edges(mesh, 1.0);
    let deformed = deform_mesh(&subdivided, &field);
    let ctx = NonplanarContext {
        field: DeformFieldSnapshot {
            band_thickness: band,
            ramp_thickness: ramp,
            deformed_top_z: field.deformed_top_z,
            nominal_layer: nominal_layer_height,
            top_islands: islands,
        },
        nominal_layer_height,
        max_angle_deg: options.max_angle_deg,
    };
    (deformed, ctx)
}

pub fn merge_meshes(meshes: &[Mesh]) -> Mesh {
    merge_meshes_internal(meshes)
}

/// Rebuild an in-memory field from mesh + options (post-pass on same mesh before deform).
pub fn rebuild_field(mesh: &Mesh, options: &NonplanarOptions, nominal_layer_height: f64) -> DeformField {
    let band = options.effective_band_thickness(nominal_layer_height);
    let ramp = options.effective_ramp_thickness(nominal_layer_height);
    let grid = build_height_field(mesh, options.grid_step_mm, options.max_angle_deg, band);
    build_deform_field(grid, band, ramp, nominal_layer_height, options.max_angle_deg)
}

/// Post-pass on deformed-mesh G-code from CuraEngine.
///
/// `planar_baseline_gcode` must be a **planar** slice of the same job on the undeformed
/// mesh (physical machine coordinates), used only for below-band equivalence.
pub fn apply_post_pass(
    deformed_slice_gcode: &str,
    planar_baseline_gcode: &str,
    mesh: &Mesh,
    options: &NonplanarOptions,
    nominal_layer_height: f64,
    printer_id: &str,
    limits: MachineLimits,
) -> (String, NonplanarReport) {
    let envelope = head_envelope_for_printer(printer_id).unwrap_or(bambu_a1_envelope());
    let mut field = rebuild_field(mesh, options, nominal_layer_height);
    let mut report = flatten_on_collision(
        &mut field,
        deformed_slice_gcode,
        envelope,
        options.max_angle_deg,
    );
    report.notes.push("ironing disabled for curved-top pass".into());
    report.top_islands = count_top_islands(&field.grid);
    let mapped = enforce_slope_safety(
        &mut field,
        deformed_slice_gcode,
        nominal_layer_height,
        &mut report,
    );
    let band_floor = field.global_band_floor_z();
    let grafted = graft_planar_shell_below_band(planar_baseline_gcode, &mapped, band_floor);
    let equiv = check_planar_equivalence(planar_baseline_gcode, &grafted, band_floor);
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
    let safety = validate_gcode(
        &grafted,
        &field,
        envelope,
        &limits,
        equiv,
        &report.collision_adjustments,
    );
    if !safety.ok {
        report
            .notes
            .push(format!("safety report: {} violation(s)", safety.violations.len()));
    }
    report.safety = Some(safety);
    let header = format_nonplanar_header(&report);
    let (mut out, report) = ok_attach_header(grafted, &header, report);
    if let Some(safety) = report.safety.as_ref() {
        out = embed_safety_json(out, safety);
    }
    (out, report)
}

fn format_nonplanar_header(report: &NonplanarReport) -> String {
    format!(
        ";SHALE_NONPLANAR mode={} max_angle={:.1} islands={} flattened={} fallback={} equiv={} safe={}\n",
        report.mode,
        report.max_angle_deg,
        report.top_islands,
        report.islands_flattened,
        report.fallback_to_planar,
        report.planar_equivalence_ok,
        report.safety.as_ref().is_some_and(|s| s.ok)
    )
}

fn ok_attach_header(mapped: String, header: &str, mut report: NonplanarReport) -> (String, NonplanarReport) {
    let mut out = header.to_string();
    out.push_str(&mapped);
    report.notes.push("post-pass complete".into());
    (out, report)
}

/// Build a printed height map envelope self-test on finished G-code (no extrusion moves should violate).
pub fn gcode_respects_envelope(gcode: &str, printer_id: &str, max_angle_deg: f64) -> bool {
    let envelope = head_envelope_for_printer(printer_id).unwrap_or(bambu_a1_envelope());
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

/// Maximum in-layer extrusion slope for moves with Z at or above `z_min` (degrees).
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
        let (out, report) = apply_post_pass(
            planar,
            planar,
            &mesh,
            &opts,
            0.2,
            "bambu_lab_a1",
            MachineLimits {
                z_max_mm: 256.0,
                max_feed_mm_per_min: 6000.0,
            },
        );
        assert!(report.planar_equivalence_ok, "{:?}", report.notes);
        assert!(report.safety.as_ref().is_some_and(|s| s.ok), "{out}");
        assert!(out.contains(";SHALE_NONPLANAR_SAFETY"));
    }
}