//! Orchestration: pre-pass mesh deform and post-pass inverse G-code map.

use serde::{Deserialize, Serialize};
use crate::mesh::{merge_meshes as merge_meshes_internal, Mesh};

use crate::collision::{flatten_on_collision, PrintedHeightMap};
use crate::config::{NonplanarOptions, NonplanarReport};
use crate::deform::{build_deform_field, deform_mesh, subdivide_long_edges, DeformField};
use crate::envelope::bambu_a1_envelope;
use crate::height_field::{build_height_field, count_top_islands};
use crate::inverse::map_gcode_inverse;

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
    let field = build_deform_field(grid, band, ramp, nominal_layer_height);
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
    build_deform_field(grid, band, ramp, nominal_layer_height)
}

/// Post-pass on planar G-code from CuraEngine.
pub fn apply_post_pass(
    gcode: &str,
    mesh: &Mesh,
    options: &NonplanarOptions,
    nominal_layer_height: f64,
) -> (String, NonplanarReport) {
    let mut field = rebuild_field(mesh, options, nominal_layer_height);
    let mut report = flatten_on_collision(
        &mut field,
        gcode,
        bambu_a1_envelope(),
        options.max_angle_deg,
    );
    report.top_islands = field.grid.width * field.grid.height; // refined below
    report.top_islands = count_top_islands(&field.grid);
    let mapped = map_gcode_inverse(&field, gcode, nominal_layer_height);
    let header = format_nonplanar_header(&report);
    ok_attach_header(mapped, &header, report)
}

fn format_nonplanar_header(report: &NonplanarReport) -> String {
    format!(
        ";SHALE_NONPLANAR mode={} max_angle={:.1} islands={} flattened={} fallback={}\n",
        report.mode,
        report.max_angle_deg,
        report.top_islands,
        report.islands_flattened,
        report.fallback_to_planar
    )
}

fn ok_attach_header(mapped: String, header: &str, mut report: NonplanarReport) -> (String, NonplanarReport) {
    let mut out = header.to_string();
    out.push_str(&mapped);
    report.notes.push("post-pass complete".into());
    (out, report)
}

/// Build a printed height map envelope self-test on finished G-code (no extrusion moves should violate).
pub fn gcode_respects_envelope(gcode: &str, max_angle_deg: f64) -> bool {
    let envelope = bambu_a1_envelope();
    let step = 2.0;
    let mut printed = PrintedHeightMap::new([-150.0, -150.0], step, 150, 150);
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
        if line.starts_with("G1") && line.contains('E') {
            if crate::collision::move_collides(
                envelope,
                &printed,
                position[0],
                position[1],
                position[2],
                max_angle_deg,
            ) {
                return false;
            }
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