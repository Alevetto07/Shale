//! Swept toolhead envelope checks against already-printed material.
//!
//! A move is sampled along its whole segment (not just its endpoints). At each
//! sample, the beads printed by earlier moves within the head's horizontal reach
//! are compared against [`HeadEnvelope::max_material_height_at`]: material taller
//! than the envelope allows at its horizontal distance is a strike. Material at
//! or below the nozzle tip plus [`COLLISION_TOLERANCE_MM`] is the bead being laid
//! and is never an obstacle.
//!
//! The printed map has its own resolution, [`PRINTED_MAP_CELL_MM`], and covers
//! the material itself, so nothing about it depends on the deform field. Each
//! cell keeps the XY of the tallest sample that landed in it and distances are
//! measured to that sample, not to the cell centre: quantising bead centrelines
//! to a 0.35 mm field cell used to turn a neighbouring 0.42 mm bead into a
//! 0.28 mm one, inside the A1's 0.3 mm tip land, so a 10 deg band failed its own
//! safety check. At 0.1 mm cells with samples every half cell the reported
//! distance is within the sample spacing of the real one.
//!
//! Two levels of max-Z tiles (8x8 cells, then 8x8 tiles) let a query skip empty
//! regions, and any tile whose tallest sample still fits under the envelope at
//! the tile's nearest distance. That prune is valid because
//! [`HeadEnvelope::max_material_height_at`] is non-decreasing in distance, which
//! `envelope_limit_is_non_decreasing` pins for every catalogue envelope.
//!
//! Planar layers take a fast path: when the nozzle tip is at or above the
//! highest material printed so far (minus the tolerance) nothing can be in the
//! way, so the window scan is skipped entirely.

use std::f64::consts::SQRT_2;

use serde::{Deserialize, Serialize};

use crate::config::{CollisionAdjustment, NonplanarReport};
use crate::deform::DeformField;
use crate::envelope::{HeadEnvelope, COLLISION_TOLERANCE_MM};

/// Resolution of the printed-material map used by [`find_collisions`].
///
/// Deliberately independent of the deform field grid: the field says where the
/// part is, never how precisely beads are remembered. A tenth of a millimetre
/// keeps a bead 0.42 mm beside the nozzle path outside the 0.3 mm tip land
/// instead of quantising it to 0.28 mm.
pub const PRINTED_MAP_CELL_MM: f64 = 0.1;

/// Most cells a printed map may allocate (16 bytes each, ~128 MB). Larger parts
/// coarsen the cell instead of exhausting memory.
const MAX_MAP_CELLS: usize = 8_000_000;

/// Cells per side of a max-Z tile (0.8 mm at the default cell size).
const TILE: usize = 8;

/// Tiles per side of a coarse max-Z super-tile (6.4 mm at the default cell size).
const SUPER: usize = 8;

/// Slack around a block in cell units: a sample may sit half a cell diagonal
/// away from the centre of the cell that stores it.
const CELL_SLACK: f64 = SQRT_2 / 2.0;

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

/// One level of the max-Z block grid over a [`PrintedHeightMap`].
///
/// Kept as a borrowed view so the fast path can be called for either level with
/// a single signature: the tiles (32 cells) behind a sample and the super-tiles
/// (6.4 mm) behind a move.
struct MaxBlocks<'a> {
    /// Cells per block along one axis.
    cells: usize,
    cols: usize,
    rows: usize,
    maxima: &'a [f64],
}

/// Printed-height field built while sweeping moves in layer order.
///
/// Cell tops live in `z`; `sample_xy` keeps the exact position of the sample
/// that set each top, so distances are measured to material rather than to the
/// cell lattice. Raise cells through [`Self::raise`] so the max-Z blocks stay
/// valid.
#[derive(Clone, Debug, Default)]
pub struct PrintedHeightMap {
    pub origin: [f64; 2],
    pub step: f64,
    pub width: usize,
    pub height: usize,
    /// Per-cell material top.
    pub z: Vec<f64>,
    sample_xy: Vec<[f32; 2]>,
    tile_cols: usize,
    tile_rows: usize,
    tile_max: Vec<f64>,
    super_cols: usize,
    super_rows: usize,
    super_max: Vec<f64>,
    max_z: f64,
}

impl PrintedHeightMap {
    pub fn new(origin: [f64; 2], step: f64, width: usize, height: usize) -> Self {
        let tile_cols = width.div_ceil(TILE);
        let tile_rows = height.div_ceil(TILE);
        let super_cols = tile_cols.div_ceil(SUPER);
        let super_rows = tile_rows.div_ceil(SUPER);
        Self {
            origin,
            step,
            width,
            height,
            z: vec![0.0; width * height],
            sample_xy: vec![[0.0; 2]; width * height],
            tile_cols,
            tile_rows,
            tile_max: vec![0.0; tile_cols * tile_rows],
            super_cols,
            super_rows,
            super_max: vec![0.0; super_cols * super_rows],
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

    /// Record material reaching `z` at `(x, y)`, remembering the exact position.
    pub fn raise(&mut self, x: f64, y: f64, z: f64) {
        let Some((ix, iy)) = self.cell(x, y) else {
            return;
        };
        let index = iy * self.width + ix;
        if z > self.z[index] {
            self.z[index] = z;
            self.sample_xy[index] = [x as f32, y as f32];
            let tile = (iy / TILE) * self.tile_cols + ix / TILE;
            self.tile_max[tile] = self.tile_max[tile].max(z);
            let span = TILE * SUPER;
            let sup = (iy / span) * self.super_cols + ix / span;
            self.super_max[sup] = self.super_max[sup].max(z);
            self.max_z = self.max_z.max(z);
        }
    }

    /// Highest material anywhere in the map (0 for an empty map).
    pub fn max_printed_z(&self) -> f64 {
        self.max_z
    }

    /// Highest printed material within `radius` of `(x, y)`.
    pub fn sample_max_in_radius(&self, x: f64, y: f64, radius: f64) -> f64 {
        let blocks = MaxBlocks {
            cells: TILE,
            cols: self.tile_cols,
            rows: self.tile_rows,
            maxima: &self.tile_max,
        };
        self.max_within(x, y, radius.max(0.0), blocks)
    }

    /// Cheap reject bound over the coarse super-tiles (6.4 mm blocks).
    fn max_in_disk_super(&self, x: f64, y: f64, radius: f64) -> f64 {
        let blocks = MaxBlocks {
            cells: TILE * SUPER,
            cols: self.super_cols,
            rows: self.super_rows,
            maxima: &self.super_max,
        };
        self.max_within(x, y, radius, blocks)
    }

    /// Tallest entry of a max block grid whose blocks reach into `radius`.
    ///
    /// Upper bound in both directions: a block counts as soon as any cell it
    /// covers could lie inside the disk, so material that is really there is
    /// never missed.
    fn max_within(&self, x: f64, y: f64, radius: f64, blocks: MaxBlocks<'_>) -> f64 {
        let (cells_per_block, cols, rows, maxima) =
            (blocks.cells, blocks.cols, blocks.rows, blocks.maxima);
        if self.step <= 0.0 || cols == 0 || rows == 0 || maxima.is_empty() {
            return 0.0;
        }
        let block = self.step * cells_per_block as f64;
        let slack = self.step * CELL_SLACK;
        let span = (cells_per_block as f64 - 1.0) * self.step + 2.0 * slack;
        let bound = (radius + slack).powi(2);
        let bx0 = (((x - radius - slack) - self.origin[0]) / block).floor() as isize;
        let bx1 = (((x + radius + slack) - self.origin[0]) / block).ceil() as isize;
        let by0 = (((y - radius - slack) - self.origin[1]) / block).floor() as isize;
        let by1 = (((y + radius + slack) - self.origin[1]) / block).ceil() as isize;
        let mut max = 0.0f64;
        for by in by0.max(0)..=by1.min(rows as isize - 1) {
            let lo = self.origin[1] + by as f64 * block - slack;
            let dy = gap(lo, lo + span, y);
            if dy * dy > bound {
                continue;
            }
            for bx in bx0.max(0)..=bx1.min(cols as isize - 1) {
                let lo = self.origin[0] + bx as f64 * block - slack;
                let dx = gap(lo, lo + span, x);
                if dx * dx + dy * dy > bound {
                    continue;
                }
                max = max.max(maxima[by as usize * cols + bx as usize]);
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
        if self.max_in_disk_super(x, y, reach) <= floor {
            return None;
        }
        let slack = self.step * CELL_SLACK;
        let outer_sq = reach * reach;
        let tile_size = self.step * TILE as f64;
        let super_size = tile_size * SUPER as f64;
        let tile_span = (TILE - 1) as f64 * self.step + 2.0 * slack;
        let super_span = (TILE * SUPER - 1) as f64 * self.step + 2.0 * slack;
        let sx0 = (((x - reach - slack) - self.origin[0]) / super_size).floor() as isize;
        let sx1 = (((x + reach + slack) - self.origin[0]) / super_size).ceil() as isize;
        let sy0 = (((y - reach - slack) - self.origin[1]) / super_size).floor() as isize;
        let sy1 = (((y + reach + slack) - self.origin[1]) / super_size).ceil() as isize;
        // (excess over the envelope, height above tip, distance)
        let mut best: Option<(f64, f64, f64)> = None;
        for sy in sy0.max(0)..=sy1.min(self.super_rows as isize - 1) {
            let lo = self.origin[1] + sy as f64 * super_size - slack;
            let dy = gap(lo, lo + super_span, y);
            let dy_sq = dy * dy;
            if dy_sq > outer_sq {
                continue;
            }
            for sx in sx0.max(0)..=sx1.min(self.super_cols as isize - 1) {
                let top = self.super_max[sy as usize * self.super_cols + sx as usize];
                if top <= floor {
                    continue;
                }
                let lo = self.origin[0] + sx as f64 * super_size - slack;
                let dx = gap(lo, lo + super_span, x);
                let d_min_sq = dx * dx + dy_sq;
                if d_min_sq > outer_sq {
                    continue;
                }
                // The limit grows with distance, so the nearest edge of the
                // block bounds what any sample inside it may carry.
                if top <= z_tip + envelope.max_material_height_at(d_min_sq.sqrt()) {
                    continue;
                }
                let ty0 = (sy as usize * SUPER).min(self.tile_rows);
                let ty1 = ((sy as usize + 1) * SUPER).min(self.tile_rows);
                let tx0 = (sx as usize * SUPER).min(self.tile_cols);
                let tx1 = ((sx as usize + 1) * SUPER).min(self.tile_cols);
                for ty in ty0..ty1 {
                    let lo = self.origin[1] + ty as f64 * tile_size - slack;
                    let dy = gap(lo, lo + tile_span, y);
                    if dy * dy > outer_sq {
                        continue;
                    }
                    for tx in tx0..tx1 {
                        let tile = ty * self.tile_cols + tx;
                        let top = self.tile_max[tile];
                        if top <= floor {
                            continue;
                        }
                        let lo = self.origin[0] + tx as f64 * tile_size - slack;
                        let dx = gap(lo, lo + tile_span, x);
                        let d_min_sq = dx * dx + dy * dy;
                        if d_min_sq > outer_sq {
                            continue;
                        }
                        if top <= z_tip + envelope.max_material_height_at(d_min_sq.sqrt()) {
                            continue;
                        }
                        let cy0 = ty * TILE;
                        let cy1 = (cy0 + TILE).min(self.height);
                        let cx0 = tx * TILE;
                        let cx1 = (cx0 + TILE).min(self.width);
                        for cy in cy0..cy1 {
                            let row = cy * self.width;
                            for cx in cx0..cx1 {
                                let z = self.z[row + cx];
                                if z <= floor {
                                    continue;
                                }
                                let [qx, qy] = self.sample_xy[row + cx];
                                let ddx = x - qx as f64;
                                let ddy = y - qy as f64;
                                let d_sq = ddx * ddx + ddy * ddy;
                                if d_sq > outer_sq {
                                    continue;
                                }
                                let d = d_sq.sqrt();
                                let height = z - z_tip;
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
            }
        }
        best.map(|(_, height, d)| (height, d))
    }
}

/// Distance from `p` to the closed interval `[lo, hi]`.
fn gap(lo: f64, hi: f64, p: f64) -> f64 {
    if p < lo {
        lo - p
    } else if p > hi {
        p - hi
    } else {
        0.0
    }
}

/// One parsed move.
struct MoveSpan {
    from: [f64; 3],
    to: [f64; 3],
    line: usize,
    extruding: bool,
    arc: bool,
}

/// G-code move parser carrying the modal state (`G90`/`G91`, `M82`/`M83`, `G92`).
struct MoveState {
    position: [f64; 3],
    /// Last absolute E word seen (M82 mode).
    last_e: Option<f64>,
    absolute_e: bool,
    /// `G90` is absolute XYZ, `G91` adds the word to the current position.
    /// A nozzle wipe is written in relative mode (`G91` then `G1 X-35`); reading
    /// that word as an absolute coordinate reports a travel through empty space
    /// beside the brim.
    absolute_xyz: bool,
}

impl MoveState {
    fn new() -> Self {
        Self {
            position: [0.0; 3],
            last_e: None,
            absolute_e: true,
            absolute_xyz: true,
        }
    }

    fn next_move(&mut self, raw: &str, line_no: usize) -> Option<MoveSpan> {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            return None;
        }
        let cmd = line.split_whitespace().next()?;
        match cmd {
            "M82" => {
                self.absolute_e = true;
                return None;
            }
            "M83" => {
                self.absolute_e = false;
                return None;
            }
            "G90" => {
                self.absolute_xyz = true;
                return None;
            }
            "G91" => {
                self.absolute_xyz = false;
                return None;
            }
            "G92" => {
                if let Some(e) = word(line, 'E') {
                    self.last_e = Some(e);
                }
                for (axis, letter) in ['X', 'Y', 'Z'].into_iter().enumerate() {
                    if let Some(value) = word(line, letter) {
                        self.position[axis] = value;
                    }
                }
                return None;
            }
            _ => {}
        }
        let arc = matches!(cmd, "G2" | "G02" | "G3" | "G03");
        if !arc && !matches!(cmd, "G0" | "G00" | "G1" | "G01") {
            return None;
        }
        let e_word = word(line, 'E');
        let extruding = match (e_word, self.last_e, self.absolute_e) {
            (Some(e), Some(previous), true) => e - previous > 1e-9,
            (Some(e), _, _) => e > 1e-9,
            (None, _, _) => false,
        };
        if let Some(e) = e_word {
            if self.absolute_e {
                self.last_e = Some(e);
            }
        }
        let mut target = self.position;
        let mut moved = false;
        for (axis, letter) in ['X', 'Y', 'Z'].into_iter().enumerate() {
            if let Some(value) = word(line, letter) {
                target[axis] = if self.absolute_xyz {
                    value
                } else {
                    self.position[axis] + value
                };
                moved = true;
            }
        }
        if !moved && !extruding {
            return None;
        }
        let from = self.position;
        self.position = target;
        Some(MoveSpan {
            from,
            to: target,
            line: line_no,
            extruding,
            arc,
        })
    }
}

/// XY bounds of every extruding move: the only places material can exist.
fn material_bounds(gcode: &str) -> Option<([f64; 2], [f64; 2])> {
    let mut state = MoveState::new();
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut any = false;
    for (index, raw) in gcode.lines().enumerate() {
        let Some(m) = state.next_move(raw, index + 1) else {
            continue;
        };
        if !m.extruding {
            continue;
        }
        for p in [m.from, m.to] {
            lo[0] = lo[0].min(p[0]);
            lo[1] = lo[1].min(p[1]);
            hi[0] = hi[0].max(p[0]);
            hi[1] = hi[1].max(p[1]);
            any = true;
        }
    }
    any.then_some((lo, hi))
}

/// Cell size, origin and size of the printed map for a material bounding box.
///
/// Starts at [`PRINTED_MAP_CELL_MM`] and only coarsens when the part is so large
/// that the map would exceed [`MAX_MAP_CELLS`].
fn map_geometry(lo: [f64; 2], hi: [f64; 2]) -> (f64, [f64; 2], usize, usize) {
    let base = PRINTED_MAP_CELL_MM;
    let span = [
        (hi[0] - lo[0]).max(0.0) + 4.0 * base,
        (hi[1] - lo[1]).max(0.0) + 4.0 * base,
    ];
    let cell = base.max((span[0] * span[1] / MAX_MAP_CELLS as f64).sqrt());
    let origin = [lo[0] - 2.0 * cell, lo[1] - 2.0 * cell];
    let width = ((hi[0] - origin[0]) / cell).floor() as usize + 2;
    let height = ((hi[1] - origin[1]) / cell).floor() as usize + 2;
    (cell, origin, width, height)
}

/// Find every move whose swept toolhead would strike already-printed material.
///
/// `grid_origin`/`grid_step`/`width`/`height` describe the caller's deform field
/// and are kept for the post-pass caller; a degenerate grid (no field) short
/// circuits the sweep. The printed map has its own resolution,
/// [`PRINTED_MAP_CELL_MM`], and covers the material itself, so reported
/// distances do not inherit the field's cell size.
///
/// At most one hit is reported per move: the first sample along the move that
/// strikes something (the worst cell at that sample).
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
    let _ = grid_origin;
    if grid_step <= 0.0 || width == 0 || height == 0 {
        return (hits, stats);
    }
    // The map covers the material, not the field grid: a bead's distance is a
    // property of the print, not of the deform field's resolution.
    let Some((lo, hi)) = material_bounds(gcode) else {
        return (hits, stats);
    };
    let (cell, origin, map_width, map_height) = map_geometry(lo, hi);
    if map_width == 0 || map_height == 0 {
        return (hits, stats);
    }
    let mut map = PrintedHeightMap::new(origin, cell, map_width, map_height);
    // Queries sample the swept move; material is laid down every half map cell,
    // so every bead is stored at least every 0.05 mm along its length.
    let sample_step = (0.5 * grid_step).max(1e-3);
    let raise_step = 0.5 * cell;
    let mut state = MoveState::new();
    for (index, raw) in gcode.lines().enumerate() {
        let Some(m) = state.next_move(raw, index + 1) else {
            continue;
        };
        let delta = [
            m.to[0] - m.from[0],
            m.to[1] - m.from[1],
            m.to[2] - m.from[2],
        ];
        let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
        let samples = if m.arc {
            stats.arc_moves += 1;
            1
        } else {
            ((distance / sample_step).ceil() as usize).clamp(1, 200_000)
        };
        let mut hit = None;
        for step in 0..=samples {
            let t = step as f64 / samples as f64;
            let x = m.from[0] + delta[0] * t;
            let y = m.from[1] + delta[1] * t;
            let z = m.from[2] + delta[2] * t;
            if let Some((material_above_tip_mm, distance_mm)) = map.scan_point(envelope, x, y, z) {
                hit = Some(CollisionHit {
                    line: m.line,
                    x,
                    y,
                    z,
                    material_above_tip_mm,
                    distance_mm,
                    extruding: m.extruding,
                });
                break;
            }
        }
        if let Some(hit) = hit {
            hits.push(hit);
        }
        if m.extruding {
            let raises = ((distance / raise_step).ceil() as usize).clamp(1, 200_000);
            for step in 0..=raises {
                let t = step as f64 / raises as f64;
                map.raise(
                    m.from[0] + delta[0] * t,
                    m.from[1] + delta[1] * t,
                    m.from[2] + delta[2] * t,
                );
            }
            stats.extruding_moves += 1;
        } else {
            stats.travel_moves += 1;
        }
    }
    (hits, stats)
}

/// Single-point legacy check kept for [`crate::pipeline::gcode_respects_envelope`].
///
/// The authoritative check is [`find_collisions`], which sweeps whole segments;
/// this one only tests the given position. `max_angle_deg` and `extruding` are
/// accepted for API compatibility and do not gate the check: the bead being laid
/// is handled by the tip tolerance, not by skipping extruding moves.
#[allow(clippy::too_many_arguments)]
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

    #[test]
    fn distance_is_measured_to_the_sample_not_the_cell_centre() {
        // One bead laid down 0.0215 mm off the nozzle, deliberately off every
        // 0.1 mm lattice position. A map that snapped beads to cell centres (or
        // to a 0.35 mm field cell) would answer 0 here and shave the distance
        // that decides whether the bead is inside the tip land.
        let mut map = PrintedHeightMap::new([0.0, 0.0], PRINTED_MAP_CELL_MM, 400, 400);
        map.raise(20.017, 20.013, 0.4);
        let (height, d) = map
            .scan_point(bambu_a1_envelope(), 20.0, 20.0, 0.2)
            .expect("0.2 mm of material 0.02 mm from the nozzle is a strike");
        assert!((height - 0.2).abs() < 1e-9, "height {height}");
        assert!((d - 0.0214).abs() < 0.001, "distance {d}");
    }

    /// Bambu's nozzle wipe is `G91` then `G1 X-35` from the brush at the back of
    /// the bed. Reading that word as an absolute coordinate put the travel on
    /// X = -35, through printed material that is not there.
    #[test]
    fn a_relative_wipe_is_not_an_absolute_jump() {
        let envelope = bambu_a1_envelope();
        // A column at X = -35, then a park at X = 80, then a relative step of
        // -35 mm. Absolute parsing flies back through the column. Relative
        // parsing stops at X = 45, well clear of it.
        let gcode = "\
G90
G1 X-35 Y0 Z0.2 E1
G1 Z8 E2
G0 X80 Y0 Z1.3
G91
G1 X-35 F30000
";
        let hits = find_collisions(gcode, envelope, [0.0, 0.0], 1.0, 4, 4);
        assert!(
            hits.is_empty(),
            "relative wipe was reported at {:?}",
            hits.iter()
                .map(|hit| (hit.x, hit.y, hit.z))
                .collect::<Vec<_>>()
        );

        // The same travel written in absolute mode really does cross the column.
        let absolute = "\
G90
G1 X-35 Y0 Z0.2 E1
G1 Z8 E2
G0 X80 Y0 Z1.3
G1 X-35
";
        let real = find_collisions(absolute, envelope, [0.0, 0.0], 1.0, 4, 4);
        assert!(
            !real.is_empty(),
            "an absolute travel through the column must still be a collision"
        );
    }
}
