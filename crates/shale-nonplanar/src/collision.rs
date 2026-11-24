//! Swept toolhead envelope checks against already-printed material.
//!
//! A move is sampled along its whole segment (not just its endpoints). At each
//! sample, the cells printed by earlier moves within the head's horizontal reach
//! are compared against [`HeadEnvelope::max_material_height_at`]: material taller
//! than the envelope allows at that distance is a strike. Material at or below
//! the nozzle tip plus [`COLLISION_TOLERANCE_MM`] is the bead being laid and is
//! never an obstacle.
//!
//! Planar layers take a fast path: when the nozzle tip is at or above the
//! highest material printed so far (minus the tolerance) nothing can be in the
//! way, so the window scan is skipped entirely.

use serde::{Deserialize, Serialize};

use crate::config::{CollisionAdjustment, NonplanarReport};
use crate::deform::DeformField;
use crate::envelope::{HeadEnvelope, COLLISION_TOLERANCE_MM};

/// Cells per side of the coarse max-Z tiles used to skip empty regions.
const TILE: usize = 8;

/// One toolhead strike found on a move.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollisionHit {
    /// 1-based line in the G-code where the strike was first detected.
    pub line: usize,
    /// Nozzle position at that sample (machine coordinates).
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Height of the offending material above the nozzle tip.
    pub material_above_tip_mm: f64,
    /// Horizontal distance from the nozzle axis to that material.
    pub distance_mm: f64,
    /// `true` for G1 moves that extrude, `false` for travel moves.
    pub extruding: bool,
}

/// Counters gathered while sweeping G-code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollisionScanStats {
    /// G2/G3 moves, which this check samples by endpoint only.
    pub arc_moves: usize,
    /// Extruding moves swept (their path is printed afterwards).
    pub extruding_moves: usize,
    /// Travel moves swept.
    pub travel_moves: usize,
}

/// Coarse printed-height field built while sweeping moves in layer order.
#[derive(Clone, Debug, Default)]
pub struct PrintedHeightMap {
    pub origin: [f64; 2],
    pub step: f64,
    pub width: usize,
    pub height: usize,
    /// Per-cell material top. Raise cells through [`Self::raise`] so the coarse
    /// max-Z tiles stay valid.
    pub z: Vec<f64>,
    tile_cols: usize,
    tile_rows: usize,
    tile_max: Vec<f64>,
    max_z: f64,
}

impl PrintedHeightMap {
    pub fn new(origin: [f64; 2], step: f64, width: usize, height: usize) -> Self {
        Self {
            origin,
            step,
            width,
            height,
            z: vec![0.0; width * height],
            tile_cols: width.div_ceil(TILE),
            tile_rows: height.div_ceil(TILE),
            tile_max: vec![0.0; width.div_ceil(TILE) * height.div_ceil(TILE)],
            max_z: 0.0,
        }
    }

    fn cell(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        if self.step <= 0.0 || self.width == 0 || self.height == 0 {
            return None;
        }
        let ix = ((x - self.origin[0]) / self.step).round();
        let iy = ((y - self.origin[1]) / self.step).round();
        if !ix.is_finite() || !iy.is_finite() || ix < 0.0 || iy < 0.0 {
            return None;
        }
        let (ix, iy) = (ix as usize, iy as usize);
        (ix < self.width && iy < self.height).then_some((ix, iy))
    }

    pub fn raise(&mut self, x: f64, y: f64, z: f64) {
        let Some((ix, iy)) = self.cell(x, y) else {
            return;
        };
        let index = iy * self.width + ix;
        if z > self.z[index] {
            self.z[index] = z;
            let tile = (iy / TILE) * self.tile_cols + ix / TILE;
            self.tile_max[tile] = self.tile_max[tile].max(z);
            self.max_z = self.max_z.max(z);
        }
    }

    /// Highest material anywhere in the map (0 for an empty map).
    pub fn max_printed_z(&self) -> f64 {
        self.max_z
    }

    /// Highest printed material within `radius` of `(x, y)` (cell centres).
    pub fn sample_max_in_radius(&self, x: f64, y: f64, radius: f64) -> f64 {
        self.max_in_disk(x, y, radius.max(0.0))
    }

    /// Upper bound of the material height inside the disk, read through the
    /// coarse tiles so empty regions cost a handful of comparisons.
    fn max_in_disk(&self, x: f64, y: f64, radius: f64) -> f64 {
        if self.step <= 0.0 || self.tile_max.is_empty() {
            return 0.0;
        }
        let tile_size = self.step * TILE as f64;
        let half = 0.5 * self.step;
        let span = radius + half;
        let tx0 = (((x - span) - self.origin[0]) / tile_size).floor() as isize;
        let tx1 = (((x + span) - self.origin[0]) / tile_size).ceil() as isize;
        let ty0 = (((y - span) - self.origin[1]) / tile_size).floor() as isize;
        let ty1 = (((y + span) - self.origin[1]) / tile_size).ceil() as isize;
        let bound = (radius + self.step).powi(2);
        let mut max = 0.0f64;
        for ty in ty0.max(0)..=ty1.min(self.tile_rows as isize - 1) {
            let y0 = self.origin[1] + ty as f64 * tile_size - half;
            let ny = y.clamp(y0, y0 + tile_size);
            let dy = y - ny;
            for tx in tx0.max(0)..=tx1.min(self.tile_cols as isize - 1) {
                let x0 = self.origin[0] + tx as f64 * tile_size - half;
                let nx = x.clamp(x0, x0 + tile_size);
                let dx = x - nx;
                if dx * dx + dy * dy > bound {
                    continue;
                }
                max = max.max(self.tile_max[ty as usize * self.tile_cols + tx as usize]);
            }
        }
        max
    }

    /// Worst strike at one nozzle position, as `(material above tip, distance)`.
    fn scan_point(
        &self,
        envelope: &HeadEnvelope,
        x: f64,
        y: f64,
        z_tip: f64,
    ) -> Option<(f64, f64)> {
        if self.step <= 0.0 || self.width == 0 || self.height == 0 {
            return None;
        }
        // Fast path for planar layers: everything printed is at or below the tip.
        let floor = z_tip + COLLISION_TOLERANCE_MM;
        if self.max_z <= floor {
            return None;
        }
        let reach = envelope.horizontal_reach_mm();
        if !reach.is_finite() || reach <= 0.0 {
            return None;
        }
        if self.max_in_disk(x, y, reach) <= floor {
            return None;
        }
        let tile_size = self.step * TILE as f64;
        let half = 0.5 * self.step;
        // Material is stored as the bead centreline quantised to cell centres,
        // so the cell centre is the material position; anything farther than
        // the head's reach cannot touch it.
        let outer = reach;
        let outer_sq = outer * outer;
        let tx0 = (((x - outer) - self.origin[0]) / tile_size).floor() as isize;
        let tx1 = (((x + outer) - self.origin[0]) / tile_size).ceil() as isize;
        let ty0 = (((y - outer) - self.origin[1]) / tile_size).floor() as isize;
        let ty1 = (((y + outer) - self.origin[1]) / tile_size).ceil() as isize;
        // (excess over the envelope, height above tip, distance)
        let mut best: Option<(f64, f64, f64)> = None;
        for ty in ty0.max(0)..=ty1.min(self.tile_rows as isize - 1) {
            let y0 = self.origin[1] + ty as f64 * tile_size - half;
            let ny = y.clamp(y0, y0 + tile_size);
            let dy = y - ny;
            for tx in tx0.max(0)..=tx1.min(self.tile_cols as isize - 1) {
                let tile = ty as usize * self.tile_cols + tx as usize;
                if self.tile_max[tile] <= floor {
                    continue;
                }
                let x0 = self.origin[0] + tx as f64 * tile_size - half;
                let nx = x.clamp(x0, x0 + tile_size);
                let dx = x - nx;
                if dx * dx + dy * dy > outer_sq {
                    continue;
                }
                let cy0 = (ty as usize) * TILE;
                let cy1 = (cy0 + TILE).min(self.height);
                let cx0 = (tx as usize) * TILE;
                let cx1 = (cx0 + TILE).min(self.width);
                for cy in cy0..cy1 {
                    let row = cy * self.width;
                    let ddy = y - (self.origin[1] + cy as f64 * self.step);
                    for cx in cx0..cx1 {
                        if self.z[row + cx] <= floor {
                            continue;
                        }
                        let ddx = x - (self.origin[0] + cx as f64 * self.step);
                        let d = (ddx * ddx + ddy * ddy).sqrt();
                        if d > outer {
                            continue;
                        }
                        let height = self.z[row + cx] - z_tip;
                        let limit = envelope.max_material_height_at(d);
                        if height > limit {
                            let excess = height - limit;
                            if best.is_none_or(|b| excess > b.0) {
                                best = Some((excess, height, d));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(_, height, d)| (height, d))
    }
}

/// Move classification while sweeping G-code.
struct Sweep {
    position: [f64; 3],
    /// Last absolute E word seen (M82 mode).
    last_e: Option<f64>,
    absolute_e: bool,
    line_no: usize,
}

impl Sweep {
    fn new() -> Self {
        Self {
            position: [0.0; 3],
            last_e: None,
            absolute_e: true,
            line_no: 0,
        }
    }
}

/// Find every move whose swept toolhead would strike already-printed material.
///
/// `grid_origin`/`grid_step`/`width`/`height` describe the printed-height grid;
/// pass the deform field grid when checking a non-planar post-pass. At most one
/// hit is reported per move: the first sample along the move that strikes
/// something (the worst cell at that sample).
pub fn find_collisions(
    gcode: &str,
    envelope: &HeadEnvelope,
    grid_origin: [f64; 2],
    grid_step: f64,
    width: usize,
    height: usize,
) -> Vec<CollisionHit> {
    find_collisions_with_stats(gcode, envelope, grid_origin, grid_step, width, height).0
}

/// [`find_collisions`] plus the counters needed to describe the sweep in notes.
pub fn find_collisions_with_stats(
    gcode: &str,
    envelope: &HeadEnvelope,
    grid_origin: [f64; 2],
    grid_step: f64,
    width: usize,
    height: usize,
) -> (Vec<CollisionHit>, CollisionScanStats) {
    let mut hits = Vec::new();
    let mut stats = CollisionScanStats::default();
    if grid_step <= 0.0 || width == 0 || height == 0 {
        return (hits, stats);
    }
    let mut map = PrintedHeightMap::new(grid_origin, grid_step, width, height);
    let sample_step = (0.5 * grid_step).max(1e-3);
    let mut sweep = Sweep::new();
    for (index, raw) in gcode.lines().enumerate() {
        sweep.line_no = index + 1;
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some(cmd) = line.split_whitespace().next() else {
            continue;
        };
        match cmd {
            "M82" => {
                sweep.absolute_e = true;
                continue;
            }
            "M83" => {
                sweep.absolute_e = false;
                continue;
            }
            "G92" => {
                if let Some(e) = word(line, 'E') {
                    sweep.last_e = Some(e);
                }
                continue;
            }
            _ => {}
        }
        let is_arc = matches!(cmd, "G2" | "G02" | "G3" | "G03");
        if !is_arc && !matches!(cmd, "G0" | "G00" | "G1" | "G01") {
            continue;
        }
        let e_word = word(line, 'E');
        let extruding = match (e_word, sweep.last_e, sweep.absolute_e) {
            (Some(e), Some(previous), true) => e - previous > 1e-9,
            (Some(e), _, _) => e > 1e-9,
            (None, _, _) => false,
        };
        if let Some(e) = e_word {
            if sweep.absolute_e {
                sweep.last_e = Some(e);
            }
        }
        let mut target = sweep.position;
        let mut moved = false;
        for (axis, letter) in ['X', 'Y', 'Z'].into_iter().enumerate() {
            if let Some(value) = word(line, letter) {
                target[axis] = value;
                moved = true;
            }
        }
        if !moved && !extruding {
            continue;
        }
        let delta = [
            target[0] - sweep.position[0],
            target[1] - sweep.position[1],
            target[2] - sweep.position[2],
        ];
        let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
        let samples = if is_arc {
            stats.arc_moves += 1;
            1
        } else {
            ((distance / sample_step).ceil() as usize).clamp(1, 200_000)
        };
        let mut hit = None;
        for step in 0..=samples {
            let t = step as f64 / samples as f64;
            let x = sweep.position[0] + delta[0] * t;
            let y = sweep.position[1] + delta[1] * t;
            let z = sweep.position[2] + delta[2] * t;
            if let Some((material_above_tip_mm, distance_mm)) = map.scan_point(envelope, x, y, z) {
                hit = Some(CollisionHit {
                    line: sweep.line_no,
                    x,
                    y,
                    z,
                    material_above_tip_mm,
                    distance_mm,
                    extruding,
                });
                break;
            }
        }
        if let Some(hit) = hit {
            hits.push(hit);
        }
        if extruding {
            for step in 0..=samples {
                let t = step as f64 / samples as f64;
                map.raise(
                    sweep.position[0] + delta[0] * t,
                    sweep.position[1] + delta[1] * t,
                    sweep.position[2] + delta[2] * t,
                );
            }
            stats.extruding_moves += 1;
        } else {
            stats.travel_moves += 1;
        }
        sweep.position = target;
    }
    (hits, stats)
}

/// Single-point legacy check kept for [`crate::pipeline::gcode_respects_envelope`].
///
/// The authoritative check is [`find_collisions`], which sweeps whole segments;
/// this one only tests the given position. `max_angle_deg` and `extruding` are
/// accepted for API compatibility and no longer gate the result: extrusion is
/// handled by the bead tolerance, not by skipping the check.
pub fn move_collides(
    envelope: &HeadEnvelope,
    printed: &PrintedHeightMap,
    x: f64,
    y: f64,
    z_nozzle: f64,
    max_angle_deg: f64,
    extruding: bool,
) -> bool {
    let _ = (max_angle_deg, extruding);
    printed.scan_point(envelope, x, y, z_nozzle).is_some()
}

/// Reduce `field.island_strength` where moves collide; update report counters.
pub fn flatten_on_collision(
    field: &mut DeformField,
    gcode: &str,
    envelope: &HeadEnvelope,
    max_angle_deg: f64,
) -> NonplanarReport {
    let mut report = NonplanarReport {
        max_angle_deg,
        ..Default::default()
    };
    report.top_islands = crate::height_field::count_top_islands(&field.grid);

    report.envelope_id = envelope.printer_id.clone();
    if !envelope.calibrated {
        report
            .notes
            .push("collision flattening skipped (uncalibrated head envelope)".into());
        return report;
    }
    if envelope.calibration_tier.as_deref() == Some("conservative") {
        report.notes.push(
            "collision flattening uses conservative envelope; measure sock/duct per JSON".into(),
        );
    }

    let (hits, stats) = find_collisions_with_stats(
        gcode,
        envelope,
        field.grid.origin,
        field.grid.step,
        field.grid.width,
        field.grid.height,
    );
    let mut extruding_hits = 0usize;
    for hit in &hits {
        if hit.extruding {
            extruding_hits += 1;
        }
        report.collision_adjustments.push(CollisionAdjustment {
            x: hit.x,
            y: hit.y,
            z: hit.z,
            action: "dampen_island".into(),
        });
        dampen_island(field, hit.x, hit.y, 0.35);
    }
    if stats.arc_moves > 0 {
        report.notes.push(format!(
            "{} arc move(s) (G2/G3) checked by endpoint only",
            stats.arc_moves
        ));
    }
    if !hits.is_empty() {
        report.notes.push(format!(
            "{} move(s) failed envelope clearance ({} extruding, {} travel)",
            hits.len(),
            extruding_hits,
            hits.len() - extruding_hits
        ));
        report.islands_flattened = hits.len().min(report.top_islands);
    }
    if field.island_strength.iter().all(|&s| s < 0.05) {
        report.fallback_to_planar = true;
        report
            .notes
            .push("all islands flattened; output approaches planar".into());
    }
    report
}

fn dampen_island(field: &mut DeformField, x: f64, y: f64, factor: f64) {
    let fx = (x - field.grid.origin[0]) / field.grid.step;
    let fy = (y - field.grid.origin[1]) / field.grid.step;
    let ix = fx.round().clamp(0.0, field.grid.width as f64 - 1.0) as usize;
    let iy = fy.round().clamp(0.0, field.grid.height as f64 - 1.0) as usize;
    let i = iy * field.grid.width + ix;
    if let Some(s) = field.island_strength.get_mut(i) {
        *s *= factor;
    }
}

fn word(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deform::build_deform_field;
    use crate::envelope::bambu_a1_envelope;
    use crate::height_field::build_height_field;
    use crate::test_meshes::box_mesh;

    #[test]
    fn collision_detects_tall_pillar_beside_low_travel() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(10.0, 10.0, 25.0);
        assert!(move_collides(
            envelope, &printed, 12.0, 10.0, 5.0, 10.0, false
        ));
    }

    #[test]
    fn deposition_extrusion_never_collides_with_its_own_bead() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 32, 32);
        printed.raise(10.0, 10.0, 5.0);
        assert!(!move_collides(
            envelope, &printed, 10.0, 10.0, 5.0, 10.0, true
        ));
    }

    #[test]
    fn gentle_dome_travel_over_own_layer_is_clear() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(10.0, 10.0, 23.8);
        assert!(!move_collides(
            envelope, &printed, 10.0, 10.0, 24.0, 10.0, false
        ));
    }

    #[test]
    fn flatten_runs_with_conservative_a1_envelope() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let mut field = build_deform_field(grid, 2.0, 0.4, 0.2, 30.0);
        let gcode = "G0 X0 Y0 Z0.2\nG1 X10 Y10 Z0.3 E1\n";
        let report = flatten_on_collision(&mut field, gcode, bambu_a1_envelope(), 10.0);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("conservative") || n.contains("post-pass")),
            "{:?}",
            report.notes
        );
        assert_eq!(report.envelope_id, "bambu_lab_a1");
    }

    #[test]
    fn pillar_scene_produces_collision_on_travel() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(20.0, 0.0, 28.0);
        assert!(move_collides(
            envelope, &printed, 18.0, 0.0, 2.5, 10.0, false
        ));
    }

    #[test]
    fn flat_print_takes_the_fast_path() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 0.35, 300, 300);
        for step in 0..200 {
            let z = 0.2 * (step / 40 + 1) as f64;
            printed.raise(40.0, 40.0, z);
            assert!(!move_collides(
                envelope, &printed, 40.0, 40.0, z, 10.0, true
            ));
        }
        assert!((printed.max_printed_z() - 1.0).abs() < 1e-9);
    }
}
