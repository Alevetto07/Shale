//! Hard guarantee: extrusion slopes in the curved band stay within `max_angle_deg`.

use crate::config::SlopeLimitingReport;
use crate::deform::DeformField;

/// Ignore sub-millimetre horizontal hops when chaining extrusions (layer Z-steps at fixed XY).
const VERTICAL_STEP_HORIZ_MM: f64 = 0.05;
/// Reporting / validation slack above `max_angle_deg` (inverse quantization, hard clamp).
pub const SLOPE_CAP_TOLERANCE_DEG: f64 = 0.5;

fn word_f(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn segment_in_curved_band(field: &DeformField, a: [f64; 3], b: [f64; 3]) -> bool {
    point_in_curved_band_geom(field, a) && point_in_curved_band_geom(field, b)
}

/// Is this a point of the reshaped band, i.e. between the band floor and the top?
///
/// Finished G-code is physical: the map has already put the top back on the
/// model's own surface, so the band runs from `z_band` to `h_top` at that XY.
fn point_in_curved_band_geom(field: &DeformField, p: [f64; 3]) -> bool {
    match field.column_limits(p[0], p[1]) {
        Some((z_band, h_top, strength, _flat)) => {
            strength > 0.0 && p[2] >= z_band - 0.05 && p[2] <= h_top + 0.05
        }
        None => false,
    }
}

fn slope_deg(a: [f64; 3], b: [f64; 3]) -> Option<f64> {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let dz = b[2] - a[2];
    let horiz = (dx * dx + dy * dy).sqrt();
    if horiz < VERTICAL_STEP_HORIZ_MM {
        return None;
    }
    if horiz < 1e-9 {
        return None;
    }
    Some(dz.atan2(horiz).abs().to_degrees())
}

#[derive(Clone, Debug)]
struct Violation {
    x: f64,
    y: f64,
    slope_deg: f64,
}

/// Scan finished G-code and list extrusion segments in the curved band that exceed the cap.
pub fn slope_violations(gcode: &str, field: &DeformField, max_deg: f64) -> Vec<(f64, f64, f64)> {
    let limit = max_deg + SLOPE_CAP_TOLERANCE_DEG;
    let mut out = Vec::new();
    scan_violations(gcode, field, limit, &mut |v| {
        out.push((v.x, v.y, v.slope_deg))
    });
    out
}

fn scan_violations(
    gcode: &str,
    field: &DeformField,
    limit_deg: f64,
    sink: &mut impl FnMut(Violation),
) {
    let mut position = [0.0f64; 3];
    let mut prev_extrusion: Option<[f64; 3]> = None;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with("G0") && (line.contains('X') || line.contains('Y')) {
            prev_extrusion = None;
        }
        if let Some(x) = word_f(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = word_f(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = word_f(line, 'Z') {
            position[2] = z;
        }
        let extruding =
            line.starts_with("G1") && line.contains('E') && word_f(line, 'E').unwrap_or(0.0) > 1e-9;
        if !extruding {
            continue;
        }
        let point = position;
        if let Some(prev) = prev_extrusion {
            if segment_in_curved_band(field, prev, point) {
                if let Some(deg) = slope_deg(prev, point) {
                    if deg > limit_deg {
                        sink(Violation {
                            x: 0.5 * (prev[0] + point[0]),
                            y: 0.5 * (prev[1] + point[1]),
                            slope_deg: deg,
                        });
                    }
                }
            }
        }
        prev_extrusion = Some(point);
    }
}

/// Largest extrusion slope (degrees) in the curved band; `None` if no band moves.
pub fn max_curved_band_extrusion_slope_deg(gcode: &str, field: &DeformField) -> Option<f64> {
    let mut position = [0.0f64; 3];
    let mut prev_extrusion: Option<[f64; 3]> = None;
    let mut max_deg = 0.0f64;
    let mut any = false;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with("G0") && (line.contains('X') || line.contains('Y')) {
            prev_extrusion = None;
        }
        if let Some(x) = word_f(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = word_f(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = word_f(line, 'Z') {
            position[2] = z;
        }
        let extruding =
            line.starts_with("G1") && line.contains('E') && word_f(line, 'E').unwrap_or(0.0) > 1e-9;
        if !extruding {
            continue;
        }
        let point = position;
        if let Some(prev) = prev_extrusion {
            if segment_in_curved_band(field, prev, point) {
                if let Some(deg) = slope_deg(prev, point) {
                    any = true;
                    max_deg = max_deg.max(deg);
                }
            }
        }
        prev_extrusion = Some(point);
    }
    any.then_some(max_deg)
}

pub fn gcode_respects_max_angle(gcode: &str, field: &DeformField) -> bool {
    slope_violations(gcode, field, field.max_angle_deg).is_empty()
}

/// Headroom under the band floor inside which a collision hit still counts.
///
/// The curved pass edits nothing below `global_band_floor_z()`: the graft splices
/// the planar baseline there verbatim and the equivalence audit proves the two
/// agree. A hit below that line therefore already exists in the print the user
/// gets with the pass off, so it cannot be a reason to fall back — the fallback
/// would emit the very same moves. One layer of slack keeps the first band layer
/// and anything the inverse map put a hair under its own floor inside the sweep.
pub const BELOW_BAND_COLLISION_SLACK_MM: f64 = 0.2;

/// Passes the pre-slice limiter may run before it reports the field unconverged.
///
/// Each pass halves the strength of the cells that own a violating level
/// surface, so a field that can fit inside the cap at all fits long before this;
/// a field that cannot is reported unconverged and the post-pass sends the job to
/// the planar baseline instead of clamping Z behind the user's back.
pub const MAX_SLOPE_LIMIT_PASSES: usize = 24;

/// Degrees of headroom left under the cap when deciding what the band may follow.
///
/// The printed shell does not follow `z_top_smooth` exactly (inverse-map
/// quantisation, the clamp budget, the layer grid), so the footprint is drawn one
/// degree inside the cap instead of on it.
pub const FOOTPRINT_SLOPE_MARGIN_DEG: f64 = 1.0;

/// Cut the deforming footprint back to the part of the top the head may follow.
///
/// `z_top_smooth` is the surface the printed shell follows: the inverse map puts
/// every band point back on the model's own top whatever the field does to the
/// deformed mesh, so a column whose surface is steeper than the cap prints a path
/// steeper than the cap however the lift is scheduled. The only lever is not to map
/// that column and let the slicer print it planar.
///
/// This is what makes the auto footprint usable. The field smooths the top, so a
/// flat deck's rim reads as a 20-30° ramp over two cells; with one flat target per
/// island those cells lift by the island's whole height, which is a rim the
/// limiter could only remove by damping the island to zero. Cutting the rim keeps
/// the deck and leaves nothing to damp.
///
/// The offset is then Lipschitz-limited, which turns the cut into a ramp of
/// `tan(cap)` per millimetre at the new boundary instead of a vertical step: the
/// columns that carried a lift taller than their distance to the footprint edge
/// keep only the part of it they can reach across the cap.
///
/// Returns the number of columns removed from the footprint.
pub fn restrict_to_printable_top(field: &mut DeformField) -> usize {
    let limit = (field.max_angle_deg - FOOTPRINT_SLOPE_MARGIN_DEG).max(0.5);
    let width = field.grid.width;
    let height = field.grid.height;
    let step = field.grid.step.max(1e-9);
    let mut cut = 0usize;
    for iy in 0..height {
        for ix in 0..width {
            let i = iy * width + ix;
            let here = match field.grid.z_top_smooth[i] {
                Some(z) if field.island_strength[i] > 0.0 => z,
                _ => continue,
            };
            let mut slope = 0.0_f64;
            for (dx, dy) in [(1isize, 0isize), (0, 1)] {
                let (px, py) = (ix as isize + dx, iy as isize + dy);
                let (nx, ny) = (ix as isize - dx, iy as isize - dy);
                let sample = |x: isize, y: isize| -> Option<f64> {
                    if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
                        return None;
                    }
                    field.grid.z_top_smooth[y as usize * width + x as usize]
                };
                let (a, b) = (sample(px, py), sample(nx, ny));
                slope = slope.max(match (a, b) {
                    (Some(a), Some(b)) => (a - b).abs() / (2.0 * step),
                    (Some(a), None) => (a - here).abs() / step,
                    (None, Some(b)) => (b - here).abs() / step,
                    (None, None) => 0.0,
                });
            }
            if slope.atan().to_degrees() > limit {
                field.island_strength[i] = 0.0;
                cut += 1;
            }
        }
    }
    if cut > 0 {
        // No strength cliff of our own: the lift may only drop by what the cap
        // allows per millimetre, so the footprint edge is printed as a ramp.
        lipschitz_limit_lift(field);
    }
    cut
}

/// Strength at or below this counts as "the deformation was flattened away".
///
/// A field this weak maps G-code to within a few micrometres of the planar
/// slice, so the job is reported as a planar fallback rather than sold as a
/// curved-top print.
pub const PLANAR_COLLAPSE_STRENGTH: f64 = 0.02;

/// Factor by which a violating cell's flattening strength is reduced per pass.
const SLOPE_DAMP_FACTOR: f64 = 0.5;

/// Level curves sampled per grid edge; a band stretched by the deformation can
/// hold more slicer layers than the nominal band thickness suggests.
const LEVEL_SAMPLES_PER_EDGE: usize = 24;

/// The four values `DeformField` uses at one grid node.
struct Column {
    z_band: f64,
    h_top: f64,
    strength: f64,
    flat: f64,
    /// Planned underside band: the forward map is disabled there, so a level
    /// curve crossing this node is a deliberate boundary, not a slope violation.
    capped: bool,
    /// A printable top exists in this column. Outside the model the grid only
    /// holds a smoothed extrapolation, and there are no level surfaces to follow.
    material: bool,
}

impl Column {
    /// Deformed-space height this column's top is lifted to.
    fn f_top(&self) -> f64 {
        self.h_top + (self.flat - self.h_top) * self.strength
    }

}

fn column(field: &DeformField, i: usize) -> Option<Column> {
    Some(Column {
        z_band: field.grid.z_band_start[i]?,
        h_top: field.grid.z_top_smooth[i]?,
        strength: field.island_strength[i],
        flat: field.grid.z_deform_flat[i].unwrap_or(field.deformed_top_z),
        capped: field.grid.z_deform_cap[i].is_some(),
        material: field.grid.z_top[i].is_some(),
    })
}

fn node_xy(field: &DeformField, i: usize) -> (f64, f64) {
    let ix = i % field.grid.width;
    let iy = i / field.grid.width;
    (
        field.grid.origin[0] + ix as f64 * field.grid.step,
        field.grid.origin[1] + iy as f64 * field.grid.step,
    )
}

/// Worst slope of any band level surface, and the grid cells that own it.
///
/// A "level surface" is where one slicer layer lands in deformed space: the
/// slicer prints at constant `z_def`, so the physical heights it deposits at two
/// neighbouring columns, `inverse_z(z_def)`, differ by the slope the toolhead has
/// to follow. The deformed *top* sheet is planar by construction (one flat target
/// per island, or the plan's capped tilt), so only surfaces inside the band can
/// exceed the cap, plus the strength step the plan stamps at a band edge, which
/// shows up here as a jump between two columns that share a level.
pub fn level_slope_violations(field: &DeformField, layer_h: f64) -> (f64, Vec<usize>) {
    let width = field.grid.width;
    let height = field.grid.height;
    let step = field.grid.step;
    let layer = if layer_h.is_finite() && layer_h > 0.0 {
        layer_h
    } else {
        field.nominal_layer.max(0.04)
    };
    let tan_cap = field.max_angle_deg.to_radians().tan();
    let mut worst = 0.0_f64;
    let mut cells: Vec<usize> = Vec::new();
    for iy in 0..height {
        for ix in 0..width {
            let i = iy * width + ix;
            let Some(a) = column(field, i) else {
                continue;
            };
            let right = (ix + 1 < width).then(|| i + 1);
            let up = (iy + 1 < height).then(|| i + width);
            for neighbour in [right, up].into_iter().flatten() {
                let Some(b) = column(field, neighbour) else {
                    continue;
                };
                if !a.material || !b.material {
                    continue;
                }
                if a.capped || b.capped {
                    // The map is intentionally discontinuous across a planned band.
                    continue;
                }
                if (a.strength - b.strength).abs() > 0.5 {
                    // A strength cliff is a wall in the deformed mesh (a band edge the
                    // slicer prints as a vertical step), not a level surface the
                    // toolhead has to follow. Damping it would flatten the whole model
                    // to remove one wall; the post-pass verifier still rejects the
                    // G-code if the wall turns out to be a real slope.
                    continue;
                }
                let wall = step * 60f64.to_radians().tan();
                if (a.h_top - b.h_top).abs() > wall {
                    // The model's own top has a step here: a near-vertical face the
                    // slicer prints as a wall and clips the path at. Flattening cannot
                    // make a wall shallower, so it is not a level-surface violation.
                    continue;
                }
                let (xa, ya) = node_xy(field, i);
                let (xb, yb) = node_xy(field, neighbour);
                // Layers only exist where both columns carry material, so only the
                // overlap of the two bands can hold a level curve crossing both.
                //
                // Start at the *lower* ramp start, not at the band entry. The map
                // blends each column's lift over `ramp_thickness` below its band
                // floor, so two neighbours with different band starts or lifts
                // already deposit at different heights under the lower band entry.
                // Sampling from `f_band()` (the higher band entry) skipped that
                // ramp: the limiter then reported `converged` on a field whose own
                // G-code still turned a 0.061 mm move into a 26.3 deg one.
                let ramp = field.ramp_thickness.max(1e-6);
                let lo = ((a.z_band.min(b.z_band) - ramp) / layer).ceil();
                let hi = (a.f_top().min(b.f_top()) / layer).floor();
                if hi < lo {
                    continue;
                }
                let count = (hi - lo) as usize + 1;
                let stride = count.div_ceil(LEVEL_SAMPLES_PER_EDGE).max(1);
                // The ramp is only a few layers thick. A stride coarser than it
                // would step straight over the region this scan exists to cover,
                // so walk the ramp level by level and stride above it.
                let ramp_levels = (ramp / layer).ceil() as usize;
                let mut k = lo;
                let mut visited = 0usize;
                while k <= hi {
                    let z_def = k * layer;
                    let pa = field.inverse_z(xa, ya, z_def);
                    let pb = field.inverse_z(xb, yb, z_def);
                    let slope = (pa - pb).abs() / step;
                    if slope > tan_cap {
                        let deg = slope.atan().to_degrees();
                        if deg > worst {
                            worst = deg;
                        }
                        cells.push(i);
                        cells.push(neighbour);
                    }
                    k += if visited < ramp_levels {
                        1.0
                    } else {
                        stride as f64
                    };
                    visited += 1;
                }
            }
        }
    }
    cells.sort_unstable();
    cells.dedup();
    (worst, cells)
}

/// Worst slope among the field's band level surfaces, in degrees.
pub fn max_field_layer_slope_deg(field: &DeformField) -> f64 {
    level_slope_violations(field, field.nominal_layer).0
}

/// True when every band level surface is inside `max_angle_deg`.
pub fn field_respects_max_angle(field: &DeformField) -> bool {
    level_slope_violations(field, field.nominal_layer)
        .1
        .is_empty()
}

/// Make the field printable before anything is sliced.
///
/// Repeatedly halves the flattening strength of the cells that own an over-cap
/// level surface, then Lipschitz-limits the lift field so a damped cell never
/// leaves a cliff beside an undamped one. Both steps only ever lower strength, so
/// the schedule walks monotonically towards planar and the loop terminates: at
/// strength 0 the mapping is the identity and every level surface is horizontal.
///
/// The report carries the worst slope found before and after, the number of
/// passes, and the smallest remaining strength so the post-pass can decide
/// whether the result is still worth printing.
pub fn limit_field_slopes(field: &mut DeformField, layer_h: f64) -> SlopeLimitingReport {
    let layer = if layer_h.is_finite() && layer_h > 0.0 {
        layer_h
    } else {
        field.nominal_layer.max(0.04)
    };
    let cap_deg = field.max_angle_deg;
    let (worst_before, mut cells) = level_slope_violations(field, layer);
    let mut report = SlopeLimitingReport {
        cap_deg,
        passes: 0,
        converged: cells.is_empty(),
        worst_before_deg: worst_before,
        worst_after_deg: worst_before,
        cells_reduced: 0,
        min_strength: field.min_strength(),
        max_strength: field.max_strength(),
        footprint_columns: field.engaged_columns(),
    };
    let mut reduced: Vec<u32> = vec![0; field.island_strength.len()];
    let mut passes = 0usize;
    while !cells.is_empty() && passes < MAX_SLOPE_LIMIT_PASSES {
        passes += 1;
        for &i in &cells {
            field.island_strength[i] = (field.island_strength[i] * SLOPE_DAMP_FACTOR).max(0.0);
            reduced[i] = 1;
        }
        spread_damped_strength(field, &mut reduced);
        lipschitz_limit_lift(field);
        let (worst, next) = level_slope_violations(field, layer);
        report.worst_after_deg = worst;
        cells = next;
    }
    report.passes = passes;
    report.converged = cells.is_empty();
    report.cells_reduced = reduced.iter().filter(|flag| **flag != 0).count();
    report.min_strength = field.min_strength();
    report.max_strength = field.max_strength();
    report.footprint_columns = field.engaged_columns();
    report
}

/// Carry a damping decision one cell outwards so the damped patch has no cliff
/// of its own; `reduced` records every cell that was ever touched.
fn spread_damped_strength(field: &mut DeformField, reduced: &mut [u32]) {
    let width = field.grid.width;
    let height = field.grid.height;
    let snapshot = field.island_strength.clone();
    for iy in 0..height {
        for ix in 0..width {
            let i = iy * width + ix;
            if reduced[i] == 0 {
                continue;
            }
            let value = snapshot[i];
            for (dx, dy) in [(1isize, 0isize), (-1, 0), (0, 1), (0, -1)] {
                let nx = ix as isize + dx;
                let ny = iy as isize + dy;
                if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                    continue;
                }
                let j = ny as usize * width + nx as usize;
                if snapshot[j] > value {
                    field.island_strength[j] = value;
                    reduced[j] = 1;
                }
            }
        }
    }
}

/// Lower-only Lipschitz limit on the lift each column applies.
///
/// The lift is `strength * (flat - h_top)`; two neighbouring columns whose lifts
/// differ by more than `tan(cap) * step` produce a step the toolhead cannot
/// follow, so the higher lift is pulled down to its neighbour's lift plus that
/// budget. Forward and backward chamfer passes leave the largest cap-compliant
/// lift under the current schedule, in one sweep.
fn lipschitz_limit_lift(field: &mut DeformField) {
    let width = field.grid.width;
    let height = field.grid.height;
    let step = field.grid.step;
    let max_dz = field.max_angle_deg.to_radians().tan() * step;
    if max_dz <= 1e-9 {
        return;
    }
    let mut lift: Vec<f64> = vec![0.0; width * height];
    let mut base: Vec<f64> = vec![0.0; width * height];
    for i in 0..width * height {
        let Some(col) = column(field, i) else {
            continue;
        };
        base[i] = (col.flat - col.h_top).max(0.0);
        lift[i] = col.strength * base[i];
    }
    for iy in 0..height {
        for ix in 0..width {
            let i = iy * width + ix;
            let mut bound = lift[i];
            if ix > 0 {
                bound = bound.min(lift[i - 1] + max_dz);
            }
            if iy > 0 {
                bound = bound.min(lift[i - width] + max_dz);
            }
            lift[i] = bound;
        }
    }
    for iy in (0..height).rev() {
        for ix in (0..width).rev() {
            let i = iy * width + ix;
            let mut bound = lift[i];
            if ix + 1 < width {
                bound = bound.min(lift[i + 1] + max_dz);
            }
            if iy + 1 < height {
                bound = bound.min(lift[i + width] + max_dz);
            }
            lift[i] = bound;
        }
    }
    for i in 0..width * height {
        if base[i] <= 1e-9 {
            continue;
        }
        let ratio = (lift[i] / base[i]).clamp(0.0, field.island_strength[i]);
        field.island_strength[i] = ratio;
    }
}

#[cfg(test)]
mod slope_limit_tests {
    use super::*;
    use crate::deform::build_deform_field;
    use crate::height_field::build_height_field;
    use crate::test_meshes::{box_mesh, sphere_mesh};

    #[test]
    fn spherical_cap_slope_limiting_converges_within_cap() {
        let mesh = sphere_mesh(12.0);
        let mut field = build_deform_field(
            build_height_field(&mesh, 0.35, 10.0, 1.6),
            1.6,
            0.4,
            0.2,
            10.0,
        );
        let raw = max_field_layer_slope_deg(&field);
        assert!(
            raw > 10.0,
            "a spherical cap must present level surfaces steeper than the cap, got {raw:.2}°"
        );
        let report = limit_field_slopes(&mut field, 0.2);
        assert!(
            report.passes <= MAX_SLOPE_LIMIT_PASSES,
            "limiter must stay inside its pass budget, ran {}",
            report.passes
        );
        assert!(
            report.worst_after_deg <= report.cap_deg + SLOPE_CAP_TOLERANCE_DEG,
            "worst level surface {:.2}° after limiting, cap {:.2}°",
            report.worst_after_deg,
            report.cap_deg
        );
        assert!(report.worst_before_deg >= raw - 1e-6);
        assert!(
            (0.0..=1.0).contains(&report.min_strength),
            "strength must stay in [0, 1], got {}",
            report.min_strength
        );
    }

    #[test]
    fn flat_top_needs_no_limiting() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let mut field = build_deform_field(
            build_height_field(&mesh, 0.35, 10.0, 1.6),
            1.6,
            0.4,
            0.2,
            10.0,
        );
        let before = field.min_strength();
        let report = limit_field_slopes(&mut field, 0.2);
        assert_eq!(report.passes, 0, "a flat top is already inside the cap");
        assert!(report.converged);
        assert_eq!(field.min_strength(), before);
    }
}
