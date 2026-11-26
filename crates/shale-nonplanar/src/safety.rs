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
    let Some(col) = field.column_limits(p[0], p[1]) else {
        return false;
    };
    let Some(t) = field.column_transition(p[0], p[1]) else {
        return false;
    };
    // The transition below the band is curved too: a ramp that starts under
    // `z_band` is where a negative rim-referenced lift stretches the layers.
    t.lift.abs() > 1e-12 && p[2] >= t.z_band - t.ramp - 0.05 && p[2] <= col.h_top + 0.05
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
    // The blur halo has a smoothed height and no material. Leaving it in the
    // footprint pulled the rim target off the part.
    for i in 0..field.island_strength.len() {
        if field
            .grid
            .z_deform_cap
            .get(i)
            .is_some_and(|cap| cap.is_some())
        {
            continue;
        }
        if field.grid.z_top.get(i).is_none_or(|z| z.is_none()) {
            field.island_strength[i] = 0.0;
        }
    }
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
    // The chamfer runs after the rim retarget, inside `limit_field_slopes`.
    // Chamfering here, while the flat target is still the island peak, is what
    // ate the lid down to a 14 mm arch.
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
            let diag = step * std::f64::consts::SQRT_2;
            let mut neighbours: Vec<(usize, f64)> = Vec::with_capacity(4);
            if ix + 1 < width {
                neighbours.push((i + 1, step));
            }
            if iy + 1 < height {
                neighbours.push((i + width, step));
            }
            if ix + 1 < width && iy + 1 < height {
                neighbours.push((i + width + 1, diag));
            }
            if ix > 0 && iy + 1 < height {
                neighbours.push((i + width - 1, diag));
            }
            for (neighbour, distance) in neighbours {
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
                // blends each column's lift over its own ramp below the band
                // floor, and that ramp is sized from the lift — several millimetres
                // when the peak is pressed down onto the rim. Sampling only
                // `ramp_thickness` (the configured minimum) stepped over the
                // stretch the thickness guard exists to bound.
                let start_a = field
                    .column_transition(xa, ya)
                    .map(|t| t.z_band - t.ramp)
                    .unwrap_or(a.z_band);
                let start_b = field
                    .column_transition(xb, yb)
                    .map(|t| t.z_band - t.ramp)
                    .unwrap_or(b.z_band);
                let ramp_floor = start_a.min(start_b);
                let lo = (ramp_floor / layer).ceil();
                let hi = (a.f_top().min(b.f_top()) / layer).floor();
                if hi < lo {
                    continue;
                }
                let count = (hi - lo) as usize + 1;
                let stride = count.div_ceil(LEVEL_SAMPLES_PER_EDGE).max(1);
                // Walk the whole transition level by level. A stride coarser than
                // it would step over the region this scan exists to cover.
                let trans_top = a.z_band.max(b.z_band);
                let ramp_levels = ((trans_top - ramp_floor) / layer).ceil().max(0.0) as usize;
                let mut k = lo;
                let mut visited = 0usize;
                while k <= hi {
                    let z_def = k * layer;
                    let pa = field.inverse_z(xa, ya, z_def);
                    let pb = field.inverse_z(xb, yb, z_def);
                    let slope = (pa - pb).abs() / distance;
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

/// Physical layer thickness the map deposits, worst case, and how many samples
/// leave the slicer's window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayerThicknessScan {
    /// Band samples taken (zero when nothing lifts).
    pub samples: usize,
    /// Thinnest physical layer found, in millimetres.
    pub min_mm: f64,
    /// Thickest physical layer found, in millimetres.
    pub max_mm: f64,
    /// Samples outside `[min_layer - tolerance, max_layer + tolerance]`.
    pub violations: usize,
    /// The worst offending thickness, for the report.
    pub worst_mm: Option<f64>,
}

/// Tolerance on the physical layer window, in millimetres.
///
/// The scan measures a Jacobian, not a bead: a sample can straddle the band
/// floor, where one deformed layer is part sheared and part ramped. Anything
/// below the slicer's own layer quantisation would reject that boundary.
pub const LAYER_THICKNESS_TOLERANCE_MM: f64 = 0.005;

/// Worst physical layer thickness the field implies, column by column.
///
/// One deformed layer is `nominal_layer` tall, and the inverse map sends it to
/// `nominal_layer * r` of physical Z with `r = dz_phys / dz_def`. The transition
/// below each band is where that bites: a fixed ramp under a multi-millimetre
/// lift used to deposit layers of a few hundredths of a millimetre, and nothing
/// rejected the file.
pub fn physical_layer_thickness(field: &DeformField) -> LayerThicknessScan {
    let nominal = field.nominal_layer.max(1e-6);
    let (r_min, r_max) = field.jacobian_bounds();
    let min_layer = r_min * nominal;
    let max_layer = r_max * nominal;
    let mut scan = LayerThicknessScan {
        samples: 0,
        min_mm: nominal,
        max_mm: nominal,
        violations: 0,
        worst_mm: None,
    };
    let mut lowest = f64::INFINITY;
    let mut highest = f64::NEG_INFINITY;
    let mut worst_gap = 0.0_f64;
    for i in 0..field.island_strength.len() {
        if field.grid.z_top.get(i).is_none_or(|z| z.is_none()) {
            continue;
        }
        if field
            .grid
            .z_deform_cap
            .get(i)
            .is_some_and(|cap| cap.is_some())
        {
            continue;
        }
        let (x, y) = node_xy(field, i);
        let Some(col) = field.column_limits(x, y) else {
            continue;
        };
        let Some(t) = field.column_transition(x, y) else {
            continue;
        };
        if col.lift.abs() <= 1e-12 || col.strength <= 1e-12 || t.lift.abs() <= 1e-12 {
            continue;
        }
        let band = col.z_band;
        let ramp_start = t.z_band - t.ramp;
        let samples = [
            ramp_start + 0.25 * t.ramp,
            ramp_start + 0.50 * t.ramp,
            ramp_start + 0.75 * t.ramp,
            band + 0.5 * nominal,
            0.5 * (band + col.h_top.max(band + nominal)),
            (col.h_top - 0.5 * nominal).max(band + 0.5 * nominal),
        ];
        let dz = nominal.min(t.ramp / 8.0).max(1e-4);
        for z in samples {
            let z = z.max(0.0);
            let dz_def = field.forward_z(x, y, z + dz) - field.forward_z(x, y, z);
            scan.samples += 1;
            if dz_def <= 1e-9 {
                scan.violations += 1;
                lowest = lowest.min(0.0);
                if worst_gap < min_layer {
                    worst_gap = min_layer;
                    scan.worst_mm = Some(0.0);
                }
                continue;
            }
            let thickness = nominal * dz / dz_def;
            lowest = lowest.min(thickness);
            highest = highest.max(thickness);
            let gap = if thickness < min_layer {
                min_layer - thickness
            } else if thickness > max_layer {
                thickness - max_layer
            } else {
                0.0
            };
            if gap > LAYER_THICKNESS_TOLERANCE_MM {
                scan.violations += 1;
                if gap > worst_gap {
                    worst_gap = gap;
                    scan.worst_mm = Some(thickness);
                }
            }
        }
    }
    if scan.samples > 0 && lowest.is_finite() {
        scan.min_mm = lowest;
        scan.max_mm = highest.max(lowest);
    }
    scan
}

/// Lengthen transition ramps until their level surfaces fit the cap.
///
/// A floor smoothed at `tan(cap)` can still leave a level in the middle of the
/// blend a degree or two over the cap, and the strength damper then eats a
/// legal dome. Tightening the floor (never shortening a Jacobian ramp) removes
/// that excess without giving up lift. Returns the scan after the last attempt.
fn settle_transition_ramps(field: &mut DeformField, layer: f64) -> (f64, Vec<usize>) {
    field.smooth_transition_ramps();
    let mut prepared = level_slope_violations(field, layer);
    let mut margin = 0.98_f64;
    while !prepared.1.is_empty() && margin > 0.12 {
        margin *= 0.75;
        field.smooth_transition_ramps_with(margin);
        prepared = level_slope_violations(field, layer);
    }
    prepared
}

/// Make the field printable before anything is sliced.
///
/// Sizes each column's transition, chamfers the lift so a rim-referenced dome
/// stays inside the cap, then halves the strength of any cell that still owns
/// an over-cap level surface. Every step only lowers strength, so the schedule
/// walks towards planar and the loop terminates.
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
    // Measured on the field as it arrived, before the transition pass moves
    // anything. Callers compare this with a slope taken on that same field.
    let (worst_before, _) = level_slope_violations(field, layer);
    let mut attenuated = field.attenuate_transitions();
    lipschitz_limit_lift(field);
    attenuated += field.attenuate_transitions();
    let (prepared, mut cells) = settle_transition_ramps(field, layer);
    let mut report = SlopeLimitingReport {
        cap_deg,
        passes: 0,
        converged: cells.is_empty(),
        worst_before_deg: worst_before.max(prepared),
        worst_after_deg: prepared,
        cells_reduced: 0,
        min_strength: field.min_strength(),
        max_strength: field.max_strength(),
        footprint_columns: field.engaged_columns(),
        thickness_attenuated_columns: attenuated,
    };
    let mut reduced: Vec<u32> = vec![0; field.island_strength.len()];
    let mut passes = 0usize;
    while !cells.is_empty() && passes < MAX_SLOPE_LIMIT_PASSES {
        passes += 1;
        for &i in &cells {
            // A planned underside already has the tilt the plan asked for.
            // Halving it is what collapsed the bridge wedge from 4.32 mm to a
            // 1 mm plateau.
            if field
                .grid
                .z_deform_cap
                .get(i)
                .is_some_and(|cap| cap.is_some())
            {
                continue;
            }
            field.island_strength[i] = (field.island_strength[i] * SLOPE_DAMP_FACTOR).max(0.0);
            reduced[i] = 1;
        }
        spread_damped_strength(field, &mut reduced);
        lipschitz_limit_lift(field);
        attenuated += field.attenuate_transitions();
        let (worst, next) = settle_transition_ramps(field, layer);
        report.worst_after_deg = worst;
        cells = next;
    }
    report.passes = passes;
    report.converged = cells.is_empty();
    report.cells_reduced = reduced.iter().filter(|flag| **flag != 0).count();
    report.min_strength = field.min_strength();
    report.max_strength = field.max_strength();
    report.footprint_columns = field.engaged_columns();
    report.thickness_attenuated_columns = attenuated;
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
                // Spreading into a planned underside band was damping a legal
                // affine wedge because a neighbour outside the band had been halved.
                if field
                    .grid
                    .z_deform_cap
                    .get(j)
                    .is_some_and(|cap| cap.is_some())
                {
                    continue;
                }
                if snapshot[j] > value {
                    field.island_strength[j] = value;
                    reduced[j] = 1;
                }
            }
        }
    }
}

/// Lipschitz limit on the signed lift each column applies.
///
/// Positive lift is pulled down and negative lift is pulled up, each only as
/// far as `tan(cap)` per millimetre, and only towards zero. Two chamfer passes
/// are the fixed point of that bound. Planned underside columns are left
/// alone: their affine is the tilt the plan asked for, and chamfering it
/// against the zero cells outside the band is what flattened a 4.32 mm wedge.
fn lipschitz_limit_lift(field: &mut DeformField) {
    let width = field.grid.width;
    let height = field.grid.height;
    let n = width * height;
    let step = field.grid.step;
    let max_dz = field.max_angle_deg.to_radians().tan() * step;
    if max_dz <= 1e-9 {
        return;
    }
    let mut lift = vec![0.0; n];
    let mut base = vec![0.0; n];
    let mut live = vec![false; n];
    let mut constrains = vec![false; n];
    for i in 0..n {
        // A planned underside keeps the affine the plan wrote. Air beside the
        // footprint does not constrain either: nothing is printed there, and
        // treating it as a zero lift chamfered a legal dome down to the cap
        // just to meet a boundary the toolhead never crosses.
        if field
            .grid
            .z_deform_cap
            .get(i)
            .is_some_and(|cap| cap.is_some())
        {
            continue;
        }
        let Some(col) = column(field, i) else {
            continue;
        };
        if !col.material {
            continue;
        }
        constrains[i] = true;
        base[i] = col.flat - col.h_top;
        lift[i] = col.strength * base[i];
        live[i] = true;
    }
    let mut positive: Vec<f64> = lift.iter().map(|value| value.max(0.0)).collect();
    let mut negative: Vec<f64> = lift.iter().map(|value| (-value).max(0.0)).collect();
    chamfer_magnitude(&mut positive, &live, &constrains, width, height, max_dz);
    chamfer_magnitude(&mut negative, &live, &constrains, width, height, max_dz);
    for i in 0..n {
        if !live[i] || base[i].abs() <= 1e-9 {
            continue;
        }
        let target = if lift[i] >= 0.0 {
            positive[i].clamp(0.0, lift[i])
        } else {
            (-negative[i]).clamp(lift[i], 0.0)
        };
        let ratio = (target / base[i]).clamp(0.0, field.island_strength[i]);
        field.island_strength[i] = ratio;
    }
}

/// Pull magnitudes down so neighbours differ by at most `max_dz`.
///
/// Cells that are not live contribute 0 when they constrain (the footprint
/// edge) and are ignored when they do not (a planned underside band).
fn chamfer_magnitude(
    values: &mut [f64],
    live: &[bool],
    constrains: &[bool],
    width: usize,
    height: usize,
    max_dz: f64,
) {
    let neighbour = |values: &[f64], j: usize| -> Option<f64> {
        if !constrains[j] {
            return None;
        }
        Some(if live[j] { values[j] } else { 0.0 })
    };
    // Diagonal neighbours are `step * sqrt(2)` apart, so they may differ by
    // that much and still sit on the cap. An orthogonal-only chamfer left
    // diagonal toolpaths about 4° over it.
    let diag = max_dz * std::f64::consts::SQRT_2;
    let at = |ix: usize, iy: usize| iy * width + ix;
    for iy in 0..height {
        for ix in 0..width {
            let i = at(ix, iy);
            if !live[i] {
                continue;
            }
            let mut bound = values[i];
            if ix > 0 {
                if let Some(other) = neighbour(values, at(ix - 1, iy)) {
                    bound = bound.min(other + max_dz);
                }
            }
            if iy > 0 {
                if let Some(other) = neighbour(values, at(ix, iy - 1)) {
                    bound = bound.min(other + max_dz);
                }
            }
            if ix > 0 && iy > 0 {
                if let Some(other) = neighbour(values, at(ix - 1, iy - 1)) {
                    bound = bound.min(other + diag);
                }
            }
            if ix + 1 < width && iy > 0 {
                if let Some(other) = neighbour(values, at(ix + 1, iy - 1)) {
                    bound = bound.min(other + diag);
                }
            }
            values[i] = bound;
        }
    }
    for iy in (0..height).rev() {
        for ix in (0..width).rev() {
            let i = at(ix, iy);
            if !live[i] {
                continue;
            }
            let mut bound = values[i];
            if ix + 1 < width {
                if let Some(other) = neighbour(values, at(ix + 1, iy)) {
                    bound = bound.min(other + max_dz);
                }
            }
            if iy + 1 < height {
                if let Some(other) = neighbour(values, at(ix, iy + 1)) {
                    bound = bound.min(other + max_dz);
                }
            }
            if ix + 1 < width && iy + 1 < height {
                if let Some(other) = neighbour(values, at(ix + 1, iy + 1)) {
                    bound = bound.min(other + diag);
                }
            }
            if ix > 0 && iy + 1 < height {
                if let Some(other) = neighbour(values, at(ix - 1, iy + 1)) {
                    bound = bound.min(other + diag);
                }
            }
            values[i] = bound;
        }
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
