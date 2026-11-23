//! Swept toolhead envelope check against already-printed material.
//!
//! Every move is checked, not just travels: an extrusion into a wall beside the
//! part hits the duct or the carriage exactly like a travel does, and ignoring
//! extrusion moves was how a nozzle strike could pass verification. The nozzle
//! tip may touch the material it is laying, so only material that stands more
//! than [`CONTACT_TOL_MM`] above the tip counts as an obstacle.

use crate::envelope::HeadEnvelope;

/// Material within this height of the nozzle tip is being laid, not struck.
pub const CONTACT_TOL_MM: f64 = 0.05;

/// A single envelope strike, kept so the report can say where and how badly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionHit {
    /// Nozzle tip position of the offending sample.
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Top of the printed material that was struck.
    pub material_z: f64,
    /// Horizontal distance from the nozzle axis to that material.
    pub distance: f64,
    /// Envelope reach at the material's height above the tip.
    pub reach: f64,
}

impl CollisionHit {
    /// Height of the struck material above the tip, in millimetres.
    pub fn height_above_tip(&self) -> f64 {
        self.material_z - self.z
    }
}

/// Coarse printed-height field built while sweeping moves in layer order.
///
/// The per-cell array is backed by a block-max index (`BLOCK` cells square) so a
/// collision query can skip whole blocks of material that stands below the
/// nozzle tip instead of scanning the full head footprint for every sample.
#[derive(Clone, Debug)]
pub struct PrintedHeightMap {
    pub origin: [f64; 2],
    pub step: f64,
    pub width: usize,
    pub height: usize,
    pub z: Vec<f64>,
    blocks_w: usize,
    blocks_h: usize,
    block_max: Vec<f64>,
}

const BLOCK: usize = 8;

impl Default for PrintedHeightMap {
    fn default() -> Self {
        Self::new([0.0, 0.0], 1.0, 1, 1)
    }
}

impl PrintedHeightMap {
    pub fn new(origin: [f64; 2], step: f64, width: usize, height: usize) -> Self {
        let blocks_w = width.div_ceil(BLOCK).max(1);
        let blocks_h = height.div_ceil(BLOCK).max(1);
        Self {
            origin,
            step,
            width,
            height,
            z: vec![0.0; width * height],
            blocks_w,
            blocks_h,
            block_max: vec![0.0; blocks_w * blocks_h],
        }
    }

    fn index(&self, x: f64, y: f64) -> Option<usize> {
        let ix = ((x - self.origin[0]) / self.step).round() as isize;
        let iy = ((y - self.origin[1]) / self.step).round() as isize;
        if ix < 0 || iy < 0 {
            return None;
        }
        let ix = ix as usize;
        let iy = iy as usize;
        if ix >= self.width || iy >= self.height {
            return None;
        }
        Some(iy * self.width + ix)
    }

    pub fn raise(&mut self, x: f64, y: f64, z: f64) {
        if let Some(i) = self.index(x, y) {
            if z > self.z[i] {
                self.z[i] = z;
                let ix = i % self.width;
                let iy = i / self.width;
                let b = (iy / BLOCK) * self.blocks_w + (ix / BLOCK);
                if z > self.block_max[b] {
                    self.block_max[b] = z;
                }
            }
        }
    }

    /// Rasterise an extruded segment, not just its endpoint: a long extrusion
    /// otherwise leaves the middle of the bead invisible to later checks.
    pub fn raise_segment(&mut self, from: [f64; 2], to: [f64; 2], z: f64, sample_step: f64) {
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let length = (dx * dx + dy * dy).sqrt();
        let step = sample_step.max(1e-3);
        let samples = (length / step).ceil().max(1.0) as usize;
        self.raise(from[0], from[1], z);
        for i in 1..=samples {
            let t = i as f64 / samples as f64;
            self.raise(from[0] + dx * t, from[1] + dy * t, z);
        }
    }

    pub fn max_in_disc(&self, x: f64, y: f64, radius: f64) -> f64 {
        let ri = (radius / self.step).ceil() as isize;
        let fx = ((x - self.origin[0]) / self.step).round() as isize;
        let fy = ((y - self.origin[1]) / self.step).round() as isize;
        let mut max_z = 0.0f64;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let ix = fx + dx;
                let iy = fy + dy;
                if ix < 0 || iy < 0 {
                    continue;
                }
                let ix = ix as usize;
                let iy = iy as usize;
                if ix >= self.width || iy >= self.height {
                    continue;
                }
                let dist =
                    ((dx as f64 * self.step).powi(2) + (dy as f64 * self.step).powi(2)).sqrt();
                if dist <= radius + self.step {
                    max_z = max_z.max(self.z[iy * self.width + ix]);
                }
            }
        }
        max_z
    }

    /// Exact test: is any printed cell inside the stepped head profile?
    ///
    /// Cells are treated as squares, so the distance is measured to the nearest
    /// edge of the cell — the map rounds positions to cell centres and must not
    /// lose a collision to that rounding. Blocks whose material stands at or
    /// below the tip are skipped whole; that is what keeps a full-G-code sweep
    /// with a 26 mm duct reach affordable.
    pub fn strike_at(
        &self,
        envelope: &HeadEnvelope,
        x: f64,
        y: f64,
        z_nozzle: f64,
        tol: f64,
    ) -> Option<CollisionHit> {
        let reach_max = envelope.max_reach_radius();
        let ri = (reach_max / self.step).ceil() as isize + 1;
        let fx = ((x - self.origin[0]) / self.step).round() as isize;
        let fy = ((y - self.origin[1]) / self.step).round() as isize;
        let tip_floor = z_nozzle + tol;
        let bx0 = ((fx - ri).max(0) as usize / BLOCK).min(self.blocks_w - 1);
        let bx1 = ((fx + ri).max(0) as usize / BLOCK).min(self.blocks_w - 1);
        let by0 = ((fy - ri).max(0) as usize / BLOCK).min(self.blocks_h - 1);
        let by1 = ((fy + ri).max(0) as usize / BLOCK).min(self.blocks_h - 1);
        let mut worst: Option<CollisionHit> = None;
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                if self.block_max[by * self.blocks_w + bx] <= tip_floor {
                    continue;
                }
                let ix0 = (bx * BLOCK) as isize;
                let iy0 = (by * BLOCK) as isize;
                for iy in iy0..(iy0 + BLOCK as isize).min(self.height as isize) {
                    for ix in ix0..(ix0 + BLOCK as isize).min(self.width as isize) {
                        let material_z = self.z[iy as usize * self.width + ix as usize];
                        if material_z <= tip_floor {
                            continue;
                        }
                        let dx = ix - fx;
                        let dy = iy - fy;
                        if dx.abs() > ri || dy.abs() > ri {
                            continue;
                        }
                        let h = material_z - z_nozzle;
                        let centre =
                            ((dx as f64 * self.step).powi(2) + (dy as f64 * self.step).powi(2))
                                .sqrt();
                        let distance = (centre - self.step * 0.5).max(0.0);
                        let reach = envelope.radius_at_z(h);
                        if distance <= reach {
                            let hit = CollisionHit {
                                x,
                                y,
                                z: z_nozzle,
                                material_z,
                                distance,
                                reach,
                            };
                            if worst.is_none_or(|w| hit.material_z > w.material_z) {
                                worst = Some(hit);
                            }
                        }
                    }
                }
            }
        }
        worst
    }
}

/// Point test against the stepped head profile. Extruding moves are checked
/// exactly like travels; touching the bead under the tip is not a strike.
pub fn move_collides(
    envelope: &HeadEnvelope,
    printed: &PrintedHeightMap,
    x: f64,
    y: f64,
    z_nozzle: f64,
) -> bool {
    printed
        .strike_at(envelope, x, y, z_nozzle, CONTACT_TOL_MM)
        .is_some()
}

/// Sample a whole segment at `sample_step` (the height-field grid step), so a
/// long move cannot jump over the head of a printed feature.
pub fn segment_collides(
    envelope: &HeadEnvelope,
    printed: &PrintedHeightMap,
    from: [f64; 3],
    to: [f64; 3],
    sample_step: f64,
    tol: f64,
) -> Option<CollisionHit> {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let length = (dx * dx + dy * dy).sqrt();
    let step = sample_step.max(1e-3);
    let samples = (length / step).ceil().max(1.0) as usize;
    for i in 0..=samples {
        let t = i as f64 / samples as f64;
        let x = from[0] + dx * t;
        let y = from[1] + dy * t;
        let z = from[2] + (to[2] - from[2]) * t;
        if let Some(hit) = printed.strike_at(envelope, x, y, z, tol) {
            return Some(hit);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::bambu_a1_envelope;

    #[test]
    fn tall_pillar_beside_a_low_travel_is_a_strike() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(10.0, 10.0, 25.0);
        assert!(move_collides(envelope, &printed, 12.0, 10.0, 5.0));
    }

    #[test]
    fn depositing_on_the_bead_below_the_tip_is_not_a_strike() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 32, 32);
        printed.raise(10.0, 10.0, 5.0);
        assert!(!move_collides(envelope, &printed, 10.0, 10.0, 5.0));
        // Even an extrusion into a fresh bead is fine: it is the same layer.
        assert!(!move_collides(envelope, &printed, 10.0, 10.0, 5.04));
    }

    #[test]
    fn extruding_into_a_tall_wall_is_a_strike() {
        // The regression this guards: extruding moves used to return early and
        // were never checked, so a nozzle could plough through a neighbouring
        // model while printing.
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(12.0, 10.0, 30.0);
        assert!(move_collides(envelope, &printed, 10.0, 10.0, 2.0));
    }

    #[test]
    fn travel_over_the_previous_layer_is_clear() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(10.0, 10.0, 23.8);
        assert!(!move_collides(envelope, &printed, 10.0, 10.0, 24.0));
    }

    #[test]
    fn segment_sampling_catches_a_strike_between_the_endpoints() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        // A 30 mm tall wall at x=20 that both endpoints clear (> max reach) but
        // the segment itself crosses.
        for y in 9..12 {
            printed.raise(20.0, y as f64, 30.0);
        }
        let hit = segment_collides(
            envelope,
            &printed,
            [-10.0, 10.0, 2.0],
            [50.0, 10.0, 2.0],
            1.0,
            CONTACT_TOL_MM,
        );
        assert!(hit.is_some(), "endpoint-only checks would miss this");
        // The endpoints alone pass: the wall is further away than any reach.
        assert!(!move_collides(envelope, &printed, -10.0, 10.0, 2.0));
        assert!(!move_collides(envelope, &printed, 50.0, 10.0, 2.0));
    }

    #[test]
    fn raise_segment_fills_the_middle_of_a_bead() {
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 0.5, 80, 80);
        printed.raise_segment([0.0, 0.0], [20.0, 0.0], 4.0, 0.5);
        assert!(printed.max_in_disc(10.0, 0.0, 0.5) > 3.9);
    }

    #[test]
    fn strike_reports_the_obstacle_height() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(20.0, 0.0, 28.0);
        let hit = printed
            .strike_at(envelope, 18.0, 0.0, 2.5, CONTACT_TOL_MM)
            .expect("strike");
        assert!((hit.material_z - 28.0).abs() < 1e-9);
        assert!((hit.height_above_tip() - 25.5).abs() < 1e-9);
    }
}
