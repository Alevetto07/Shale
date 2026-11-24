//! Mesh deformation for the curved-top band (ramp + band, monotone in Z).

use crate::mesh::Mesh;
use serde::{Deserialize, Serialize};

use crate::height_field::HeightGrid;

/// How one column maps: the shear lift and the physical transition below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    /// Physical Z where the column's band starts (the shear applies above this).
    pub z_band: f64,
    /// Vertical displacement applied above `z_band`. Negative presses a peak down
    /// onto the rim; positive is the legacy upward lift.
    pub lift: f64,
    /// Physical thickness of the blend between untouched plastic and `z_band`.
    pub ramp: f64,
}

/// Maps physical coordinates to the deformed space used for planar slicing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeformField {
    pub grid: HeightGrid,
    pub band_thickness: f64,
    pub ramp_thickness: f64,
    /// Z of the flat top in deformed space.
    pub deformed_top_z: f64,
    pub min_layer: f64,
    pub max_layer: f64,
    pub nominal_layer: f64,
    /// Island-wise flattening strength in \[0, 1\]; 1 = full bend.
    pub island_strength: Vec<f64>,
    /// Slope limit applied when inverse-mapping G-code segments (degrees).
    pub max_angle_deg: f64,
    /// Connected top sheets the field flattens (one flat target each).
    #[serde(default)]
    pub top_islands: usize,
    /// Per-node transition thickness after the slope-aware lengthening pass.
    ///
    /// Empty until [`DeformField::smooth_transition_ramps`] runs. Sampling then
    /// falls back to [`DeformField::transition_thickness_for_lift`].
    #[serde(default)]
    pub node_ramp: Vec<f64>,
}

/// The band geometry and the lift [`DeformField`] applies at one XY.
///
/// `lift` is interpolated from the per-node values (see
/// [`DeformField::lift_at`]); `h_top`, `flat` and `strength` are the bilinear
/// samples callers judge the column by.
pub(crate) struct ColumnLimits {
    pub z_band: f64,
    pub h_top: f64,
    pub strength: f64,
    pub lift: f64,
}

impl DeformField {
    /// Lowest Z at which any column's deformation band starts.
    ///
    /// Below it no column can be deformed, so callers can skip the map. This is a
    /// minimum over the plate on purpose: a tall island's band floor says nothing
    /// about a low island beside it, and using the tallest top here made the inverse
    /// map skip every move under a short part of the plate.
    pub fn global_band_floor_z(&self) -> f64 {
        let mut floor = f64::INFINITY;
        for i in 0..self.grid.z_band_start.len() {
            let Some(z_band) = self.grid.z_band_start[i] else {
                continue;
            };
            let lift = self.node_lift(i);
            if lift.abs() < 1e-12 {
                continue;
            }
            let ramp = self
                .node_ramp
                .get(i)
                .copied()
                .filter(|ramp| *ramp > 1e-9)
                .unwrap_or_else(|| self.transition_thickness_for_lift(lift));
            floor = floor.min(z_band - ramp);
        }
        if floor.is_finite() {
            return floor;
        }
        let max_top = self
            .grid
            .z_top_smooth
            .iter()
            .filter_map(|z| *z)
            .fold(0.0_f64, f64::max);
        max_top - self.band_thickness - self.ramp_thickness
    }

    /// Allowed range of `r = dz_phys / dz_def` inside the map.
    ///
    /// A deformed layer is `nominal_layer` tall in the space the slicer works
    /// in; the physical thickness it deposits is `nominal_layer * r`. Both
    /// bounds come from the field's own slicer limits, so a job can never be
    /// laid down thinner than `min_layer` or thicker than `max_layer`.
    pub fn jacobian_bounds(&self) -> (f64, f64) {
        let nominal = self.nominal_layer.max(1e-6);
        // Keep 1.0 inside the range: above the band the map is a pure shear and
        // r = 1 whatever the limits say, so a nominal layer taller than
        // `max_layer` must not turn every band move into a violation.
        let r_min = (self.min_layer / nominal).clamp(1e-3, 0.999);
        let r_max = (self.max_layer / nominal).max(1.001);
        (r_min, r_max)
    }

    /// Physical transition thickness a lift of `lift` needs for `r` to stay in bounds.
    ///
    /// The transition is linear in the ramp, so `r` is constant through it:
    /// `r = ramp / (ramp + lift)`. Solving `r >= r_min` for a positive lift gives
    /// `ramp >= lift * r_min / (1 - r_min)`; solving `r <= r_max` for a negative
    /// one gives `ramp >= |lift| * r_max / (r_max - 1)`. The configured
    /// `ramp_thickness` is the floor: a short, gentle lift keeps the blend it was
    /// configured with.
    pub fn transition_thickness_for_lift(&self, lift: f64) -> f64 {
        let base = self.ramp_thickness.max(1e-6);
        let (r_min, r_max) = self.jacobian_bounds();
        if lift > 0.0 {
            (lift * r_min / (1.0 - r_min)).max(base)
        } else if lift < 0.0 {
            (lift.abs() * r_max / (r_max - 1.0)).max(base)
        } else {
            base
        }
    }

    /// Lowest Z the transition may start at: two nominal layers off the bed.
    pub fn transition_floor_z(&self) -> f64 {
        (2.0 * self.nominal_layer).max(0.0)
    }

    /// Strength a column may keep so its transition fits above the bed.
    ///
    /// `transition_thickness_for_lift` is linear in `|lift|`, so the largest
    /// strength that fits below `z_band` has a closed form. The smaller of that
    /// and the planned strength keeps the schedule continuous: the cap goes to
    /// zero exactly as the room does.
    fn fitted_strength(&self, z_band: f64, base: f64, strength: f64) -> f64 {
        let strength = strength.clamp(0.0, 1.0);
        if strength <= 0.0 || base.abs() <= 1e-12 {
            return strength;
        }
        let available = (z_band - self.transition_floor_z()).max(0.0);
        let configured = self.ramp_thickness.max(1e-6);
        if available + 1e-9 < configured {
            // Not even the configured blend fits: this column cannot be lifted
            // without starting the ramp inside the first two layers.
            return 0.0;
        }
        let (r_min, r_max) = self.jacobian_bounds();
        let coefficient = if base > 0.0 {
            r_min / (1.0 - r_min)
        } else {
            r_max / (r_max - 1.0)
        };
        let fit = available / (coefficient * base.abs());
        strength.min(fit).clamp(0.0, 1.0)
    }

    pub(crate) fn column_limits(&self, x: f64, y: f64) -> Option<ColumnLimits> {
        let z_band = self.grid.sample_band_start(x, y)?;
        let h_top = self
            .grid
            .sample_top_smooth(x, y)
            .unwrap_or(self.deformed_top_z);
        let strength = self.island_strength_at(x, y);
        Some(ColumnLimits {
            z_band,
            h_top,
            strength,
            lift: self.lift_at(x, y),
        })
    }

    /// Lift applied at one grid node: `strength * (flat - h_top)`.
    ///
    /// Nodes with no printable top, or whose band never starts, lift nothing.
    pub fn node_lift(&self, i: usize) -> f64 {
        let Some(h_top) = self.grid.z_top_smooth.get(i).copied().flatten() else {
            return 0.0;
        };
        if self.grid.z_band_start.get(i).copied().flatten().is_none() {
            return 0.0;
        }
        let flat = self
            .grid
            .z_deform_flat
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(self.deformed_top_z);
        let strength = self.island_strength.get(i).copied().unwrap_or(0.0);
        strength * (flat - h_top)
    }

    /// The lift the map applies at an XY, interpolated from the per-node lifts.
    ///
    /// The old map formed `strength(x,y) * (flat(x,y) - h_top(x,y))`, a product
    /// of two bilinear samples. That product is *not* the bilinear blend of the
    /// per-node lifts, so the slope limiter — which bounds node values — could
    /// not bound the interior of a cell. On a 0.5 mm grid a G-code move 0.061 mm
    /// long then crossed a 0.030 mm Z step (26°) on a field whose nodes all
    /// passed at ≤10°. Interpolating the lift itself makes the cell interior a
    /// convex blend of values the limiter has already capped, so a node check
    /// bounds every interior move, and `forward_z`/`inverse_z` stay exact
    /// inverses because both read this one number.
    pub fn lift_at(&self, x: f64, y: f64) -> f64 {
        let Some((ix, iy, tx, ty)) = self.grid.cell_weights(x, y) else {
            return self.edge_lift(x, y);
        };
        let w = self.grid.width;
        self.node_lift(iy * w + ix) * (1.0 - tx) * (1.0 - ty)
            + self.node_lift(iy * w + ix + 1) * tx * (1.0 - ty)
            + self.node_lift((iy + 1) * w + ix) * (1.0 - tx) * ty
            + self.node_lift((iy + 1) * w + ix + 1) * tx * ty
    }

    /// Nearest-node lift for points on the grid's last row/column or outside it,
    /// the same way the height samples fall back to their edge value.
    fn edge_lift(&self, x: f64, y: f64) -> f64 {
        let fx = (x - self.grid.origin[0]) / self.grid.step;
        let fy = (y - self.grid.origin[1]) / self.grid.step;
        if fx < 0.0 || fy < 0.0 {
            return self.node_lift(0);
        }
        let ix = (fx.round() as isize).clamp(0, self.grid.width as isize - 1) as usize;
        let iy = (fy.round() as isize).clamp(0, self.grid.height as isize - 1) as usize;
        self.node_lift(iy * self.grid.width + ix)
    }

    /// Lowest deformed Z at this XY: below it the map is the identity.
    ///
    /// The G-code side used to compare against [`Self::global_band_floor_z`],
    /// the lowest band floor anywhere on the plate. That passes through any
    /// move below the global floor even when the *local* band starts well
    /// above it, so a lifted island next to a lower one printed its upper band
    /// at un-mapped Z and the next layer descended into it. Every skip test
    /// has to be local to the column it is judged in.
    /// Lift and physical transition thickness this column actually applies.
    ///
    /// The lift is the interpolated per-node value ([`Self::lift_at`]), not the
    /// product of bilinear samples. The ramp is the smoothed per-node thickness
    /// when that pass has run, and never shorter than the lift's own Jacobian
    /// minimum. Forward, inverse and the safety scan all read this one struct.
    pub fn column_transition(&self, x: f64, y: f64) -> Option<Transition> {
        let z_band = self.grid.sample_band_start(x, y)?;
        // Interpolated per-node lift, then the closed-form cap for the room
        // under this sample's band. At a node `attenuate_transitions` has
        // already written that cap into the strength, so this is the identity
        // there and only bites between nodes.
        let raw = self.lift_at(x, y);
        let lift = if raw.abs() < 1e-12 {
            0.0
        } else {
            self.fitted_strength(z_band, raw, 1.0) * raw
        };
        Some(Transition {
            z_band,
            lift,
            ramp: self.ramp_at(x, y, lift),
        })
    }

    /// Transition thickness at an XY: smoothed node ramps, floored by the lift's
    /// own Jacobian minimum so a sample between nodes cannot crush or tear.
    fn ramp_at(&self, x: f64, y: f64, lift: f64) -> f64 {
        let sized = self.transition_thickness_for_lift(lift);
        if self.node_ramp.len() != self.island_strength.len() || self.node_ramp.is_empty() {
            return sized;
        }
        let Some((ix, iy, tx, ty)) = self.grid.cell_weights(x, y) else {
            return self.edge_ramp(x, y).max(sized);
        };
        let w = self.grid.width;
        let sample = |i: usize| {
            let ramp = self.node_ramp.get(i).copied().unwrap_or(0.0);
            (ramp > 1e-9).then_some(ramp)
        };
        let corners = [
            (sample(iy * w + ix), (1.0 - tx) * (1.0 - ty)),
            (sample(iy * w + ix + 1), tx * (1.0 - ty)),
            (sample((iy + 1) * w + ix), (1.0 - tx) * ty),
            (sample((iy + 1) * w + ix + 1), tx * ty),
        ];
        let mut sum = 0.0;
        let mut weight = 0.0;
        for (ramp, corner_w) in corners {
            if let Some(ramp) = ramp {
                sum += ramp * corner_w;
                weight += corner_w;
            }
        }
        if weight > 0.0 {
            (sum / weight).max(sized)
        } else {
            sized
        }
    }

    fn edge_ramp(&self, x: f64, y: f64) -> f64 {
        if self.node_ramp.is_empty() {
            return 0.0;
        }
        let fx = (x - self.grid.origin[0]) / self.grid.step;
        let fy = (y - self.grid.origin[1]) / self.grid.step;
        if fx < 0.0 || fy < 0.0 {
            return self.node_ramp.first().copied().unwrap_or(0.0);
        }
        let ix = (fx.round() as isize).clamp(0, self.grid.width as isize - 1) as usize;
        let iy = (fy.round() as isize).clamp(0, self.grid.height as isize - 1) as usize;
        self.node_ramp
            .get(iy * self.grid.width + ix)
            .copied()
            .unwrap_or(0.0)
    }

    pub fn local_band_floor_z(&self, x: f64, y: f64) -> Option<f64> {
        self.column_transition(x, y)
            .and_then(|t| (t.lift.abs() > 1e-12).then_some(t.z_band - t.ramp))
    }

    /// True when this G-code point lies in the curved-top shell (deformed coordinates).
    pub fn gcode_needs_inverse(&self, x: f64, y: f64, z_def: f64) -> bool {
        let Some(t) = self.column_transition(x, y) else {
            return false;
        };
        if t.lift.abs() < 1e-12 {
            return false;
        }
        let z_phys = self.inverse_z(x, y, z_def);
        // The ramp below the band is mapped too, at a fraction of the lift.
        z_phys > t.z_band - t.ramp + 1e-9 && (z_phys - z_def).abs() > 5e-3
    }

    /// Forward map: physical Z → deformed Z.
    ///
    /// Above the band floor the map is a pure vertical **shear**: the whole
    /// cross-section rises by the column's lift `strength * (flat - h_top)`, so
    /// the deformed top is flat while every level surface inside the band keeps
    /// the model's own slope. Below it, a ramp of `ramp_thickness` blends the
    /// lift to zero, so the part under the band is untouched and the map stays
    /// monotone in Z.
    ///
    /// The band used to be *rescaled* instead: `[z_band, h_top]` was stretched
    /// onto `[z_band, flat]`, so a lid whose top sat 8.4 mm below the island's
    /// highest point got a 10 mm deformed band out of a 1.6 mm physical one. The
    /// slicer then laid ~50 layers into 1.6 mm of plastic, 0.033 mm apart, and
    /// every travel move in the band ploughed through the bead it had just laid
    /// (examples/nonplanar-demo.stl: 2835 travel collisions 0.11 mm above the
    /// tip, tolerance 0.1 mm). A shear keeps the layer spacing physical, so the
    /// band prints like any other slab and the inverse map still restores the
    /// model's own surface.
    pub fn forward_z(&self, x: f64, y: f64, z: f64) -> f64 {
        // A planned underside is a tilted plane the algebraic map already
        // describes. Everywhere else the inverse is a bilinear blend of the
        // node values, and this is its inverse: the blend is monotone in Z, so
        // a search lands on the unique deformed height.
        if self.cell_has_cap(x, y) || self.on_grid_node(x, y) {
            return self.forward_z_algebraic(x, y, z);
        }
        let mut lo = z - 40.0;
        let mut hi = z + 40.0;
        for _ in 0..48 {
            let mid = 0.5 * (lo + hi);
            if self.inverse_z(x, y, mid) < z {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// Algebraic shear, using the interpolated column. Exact on a grid node and
    /// on a uniform cell; between nodes the ramp terms are nonlinear, so the
    /// public map blends node results instead of using this directly.
    fn forward_z_algebraic(&self, x: f64, y: f64, z: f64) -> f64 {
        if self
            .grid
            .sample_deform_cap(x, y)
            .is_some_and(|cap| z > cap + 1e-9)
        {
            return z;
        }
        let Some(t) = self.column_transition(x, y) else {
            return z;
        };
        if t.lift.abs() < 1e-12 {
            return z;
        }
        let ramp_start = t.z_band - t.ramp;
        if z <= ramp_start {
            return z;
        }
        if z >= t.z_band {
            return z + t.lift;
        }
        let w = (z - ramp_start) / t.ramp;
        z + w * t.lift
    }

    /// Local `dz_def / dz_phys`.
    ///
    /// Deliberately unclamped: a ratio outside `[min_layer, max_layer] /
    /// nominal` is a field that crushes or stretches the part, and the safety
    /// pass has to see the real number to reject it. Clamping here was hiding a
    /// several-fold over-extrusion in the transition.
    pub fn layer_thickness_ratio(&self, x: f64, y: f64, z0: f64, z1: f64) -> f64 {
        let dz_phys = (z1 - z0).abs().max(1e-6);
        let d0 = self.forward_z(x, y, z0);
        let d1 = self.forward_z(x, y, z1);
        (d1 - d0).abs() / dz_phys
    }

    pub fn island_strength_at(&self, x: f64, y: f64) -> f64 {
        if self.island_strength.len() == 1 {
            return self.island_strength[0];
        }
        // Same weights as the height samples: the strength is part of the same
        // column, and reading it with a bare `floor` made the inverse map judge a
        // point with one column's heights and its neighbour's strength. The lift is
        // `strength * (flat - h_top)`, so a one-cell index slip moved the mapped Z
        // by the whole local lift and the slope limiter then damped the entire
        // footprint chasing phantom level surfaces.
        let Some((ix, iy, tx, ty)) = self.grid.cell_weights(x, y) else {
            return self.island_strength.last().copied().unwrap_or(1.0).min(1.0);
        };
        let w = self.grid.width;
        let s = &self.island_strength;
        let value = s[iy * w + ix] * (1.0 - tx) * (1.0 - ty)
            + s[iy * w + ix + 1] * tx * (1.0 - ty)
            + s[(iy + 1) * w + ix] * (1.0 - tx) * ty
            + s[(iy + 1) * w + ix + 1] * tx * ty;
        value.clamp(0.0, 1.0)
    }

    /// Inverse Z of one deformed layer.
    ///
    /// Evaluated at the four surrounding nodes with the algebraic shear, then
    /// bilinearly blended. Interpolating the ramp *parameters* and then running
    /// the shear made a level that was flat on the nodes spike to 75° in the
    /// middle of a 0.35 mm cell, and the toolpath followed the spike into
    /// material it had just laid. A blend of the node heights cannot be steeper
    /// than the edges the slope scan already checked.
    pub fn inverse_z(&self, x: f64, y: f64, z_deformed: f64) -> f64 {
        if self.cell_has_cap(x, y) || self.on_grid_node(x, y) {
            return self.inverse_z_algebraic(x, y, z_deformed);
        }
        let Some((ix, iy, tx, ty)) = self.grid.cell_weights(x, y) else {
            return self.inverse_z_algebraic(x, y, z_deformed);
        };
        let w = self.grid.width;
        let at = |ix: usize, iy: usize| {
            let x = self.grid.origin[0] + ix as f64 * self.grid.step;
            let y = self.grid.origin[1] + iy as f64 * self.grid.step;
            self.inverse_z_algebraic(x, y, z_deformed)
        };
        // Air beside the mesh is not a level. A column the slope cut zeroed
        // still has a top: the chamfer already brought its neighbours down to
        // meet it, and leaving it out of the blend kept their lift constant
        // across the cell and then dropped it all on the cut node. A short
        // perimeter move crossed that step at ~20° and missed the clamp budget.
        let corner = |cix: usize, ciy: usize, weight: f64| -> Option<(f64, f64)> {
            if weight <= 1e-12 {
                return None;
            }
            let i = ciy * w + cix;
            let on_part = self.grid.z_top.get(i).is_some_and(|z| z.is_some());
            on_part.then_some((at(cix, ciy), weight))
        };
        let corners = [
            corner(ix, iy, (1.0 - tx) * (1.0 - ty)),
            corner(ix + 1, iy, tx * (1.0 - ty)),
            corner(ix, iy + 1, (1.0 - tx) * ty),
            corner(ix + 1, iy + 1, tx * ty),
        ];
        let mut sum = 0.0;
        let mut weight = 0.0;
        for (z, corner_w) in corners.into_iter().flatten() {
            sum += z * corner_w;
            weight += corner_w;
        }
        if weight <= 1e-12 {
            return self.inverse_z_algebraic(x, y, z_deformed);
        }
        sum / weight
    }

    fn on_grid_node(&self, x: f64, y: f64) -> bool {
        match self.grid.cell_weights(x, y) {
            Some((_, _, tx, ty)) => tx <= 1e-9 && ty <= 1e-9,
            None => true,
        }
    }

    /// True when any corner of the cell carries a planned underside cap.
    fn cell_has_cap(&self, x: f64, y: f64) -> bool {
        let Some((ix, iy, _, _)) = self.grid.cell_weights(x, y) else {
            return self.grid.sample_deform_cap(x, y).is_some();
        };
        let w = self.grid.width;
        [
            iy * w + ix,
            iy * w + ix + 1,
            (iy + 1) * w + ix,
            (iy + 1) * w + ix + 1,
        ]
        .into_iter()
        .any(|i| {
            self.grid
                .z_deform_cap
                .get(i)
                .is_some_and(|cap| cap.is_some())
        })
    }

    /// Algebraic inverse of the shear in [`Self::forward_z_algebraic`].
    fn inverse_z_algebraic(&self, x: f64, y: f64, z_deformed: f64) -> f64 {
        let Some(t) = self.column_transition(x, y) else {
            return z_deformed;
        };
        if t.lift.abs() < 1e-12 {
            return z_deformed;
        }
        if let Some(cap) = self.grid.sample_deform_cap(x, y) {
            let phys_est = z_deformed - t.lift;
            if phys_est > cap + 1e-9 {
                return z_deformed;
            }
        }
        let ramp_start = t.z_band - t.ramp;
        if z_deformed <= ramp_start + 1e-12 {
            return z_deformed;
        }
        if z_deformed >= t.z_band + t.lift {
            return z_deformed - t.lift;
        }
        // Inside the ramp, `z_def = z + (z - ramp_start) / ramp * lift`.
        (z_deformed + ramp_start * t.lift / t.ramp) / (1.0 + t.lift / t.ramp)
    }

    /// Worst |round-trip Z error| on a grid inside the deformation band (millimetres).
    pub fn max_round_trip_error_mm(&self, x: f64, y: f64, samples: usize) -> f64 {
        let Some(z_band) = self.grid.sample_band_start(x, y) else {
            return 0.0;
        };
        let z_top = z_band + self.band_thickness + self.ramp_thickness;
        let mut worst = 0.0f64;
        for i in 0..=samples {
            let z = z_band + (z_top - z_band) * i as f64 / samples as f64;
            let f = self.forward_z(x, y, z);
            let back = self.inverse_z(x, y, f);
            worst = worst.max((back - z).abs());
        }
        worst
    }

    pub fn verify_monotone(&self, samples: usize) -> bool {
        let (min_x, max_x) = (
            self.grid.origin[0],
            self.grid.origin[0] + (self.grid.width - 1) as f64 * self.grid.step,
        );
        let xs: Vec<f64> = (0..samples)
            .map(|i| min_x + (max_x - min_x) * i as f64 / samples as f64)
            .collect();
        for &x in &xs {
            let y = self.grid.origin[1];
            let mut prev = self.forward_z(x, y, 0.0);
            for step in 1..=samples {
                let z = step as f64 * 0.05;
                let cur = self.forward_z(x, y, z);
                if cur + 1e-9 < prev {
                    return false;
                }
                prev = cur;
            }
        }
        true
    }

    /// Recompute `deformed_top_z` and the per-island flat targets.
    ///
    /// Called again after the auto plan has tilted `z_top_smooth`: the island
    /// maximum is the height the plan asks that sheet to flatten to, so it has to
    /// be read from the tilted grid, not the raw one.
    pub fn refresh_top_targets(&mut self) {
        self.top_islands = assign_island_flat_targets(&mut self.grid);
        self.deformed_top_z = self
            .grid
            .z_top_smooth
            .iter()
            .flatten()
            .fold(0.0_f64, |acc, z| acc.max(*z));
    }

    /// Smallest flattening strength anywhere the field actually bends something.
    ///
    /// Columns without a printable top are skipped: they carry the initial 1.0
    /// and cannot lift anything, so counting them would hide a fully damped field.
    ///
    /// This is *not* the go/no-go for a curved job. In auto the deforming footprint
    /// is a subset of the top — every column outside the plan, and every column
    /// cut because its own surface is steeper than the cap, sits at exactly 0 — so
    /// the minimum is 0 for every honest auto field. Ask [`Self::max_strength`]
    /// whether anything is still lifting.
    /// A column the strength summaries should count.
    ///
    /// Real material, or a planned underside band. The blur halo has a smoothed
    /// height and no `z_top`; counting it made every honest field report a
    /// minimum of zero.
    fn counts_for_strength(&self, i: usize) -> bool {
        let smooth = self.grid.z_top_smooth.get(i).is_some_and(|z| z.is_some());
        if !smooth {
            return false;
        }
        let real = self.grid.z_top.get(i).is_some_and(|z| z.is_some());
        let capped = self.grid.z_deform_cap.get(i).is_some_and(|z| z.is_some());
        real || capped
    }

    pub fn min_strength(&self) -> f64 {
        let mut min = f64::INFINITY;
        for (i, strength) in self.island_strength.iter().enumerate() {
            if self.counts_for_strength(i) {
                min = min.min(*strength);
            }
        }
        if min.is_finite() {
            min
        } else {
            0.0
        }
    }

    /// Largest flattening strength left on a column that carries a printable top.
    ///
    /// The question a collapse decision actually asks: is there anywhere left for
    /// the band to follow the surface? A steep patch that had to be un-flattened
    /// drives its own column to 0 and leaves its neighbours alone, so this stays
    /// high while [`Self::min_strength`] drops to 0.
    pub fn max_strength(&self) -> f64 {
        let mut max = f64::NEG_INFINITY;
        for (i, strength) in self.island_strength.iter().enumerate() {
            if self.counts_for_strength(i) {
                max = max.max(*strength);
            }
        }
        if max.is_finite() {
            max
        } else {
            0.0
        }
    }

    /// Columns that carry a printable top and still lift (strength above 0).
    pub fn engaged_columns(&self) -> usize {
        self.island_strength
            .iter()
            .enumerate()
            .filter(|(i, s)| **s > 0.0 && self.counts_for_strength(*i))
            .count()
    }

    /// Columns with a printable top of their own (the real footprint size).
    pub fn material_columns(&self) -> usize {
        self.grid.z_top.iter().filter(|z| z.is_some()).count()
    }

    /// Lower every column's strength so its transition fits above the bed.
    ///
    /// The ramp each column needs is linear in `|lift|`, so the strength that
    /// fits below `z_band` has a closed form ([`Self::fitted_strength`]). The
    /// map applies that same cap at the nodes, and [`Self::lift_at`] carries it
    /// between them. Returns the number of columns whose strength had to drop.
    pub fn attenuate_transitions(&mut self) -> usize {
        let mut changed = 0usize;
        for i in 0..self.island_strength.len() {
            let (Some(z_band), Some(h_top)) = (
                self.grid.z_band_start.get(i).copied().flatten(),
                self.grid.z_top_smooth.get(i).copied().flatten(),
            ) else {
                continue;
            };
            let flat = self
                .grid
                .z_deform_flat
                .get(i)
                .copied()
                .flatten()
                .unwrap_or(self.deformed_top_z);
            let strength = self.island_strength[i];
            let fitted = self.fitted_strength(z_band, flat - h_top, strength);
            if fitted < strength - 1e-12 {
                self.island_strength[i] = fitted;
                changed += 1;
            }
        }
        changed
    }

    /// Lengthen short ramps so the transition floor stays inside the slope cap.
    ///
    /// A per-column minimum ramp meets the layer-thickness window, but that
    /// minimum changes faster than the surface: on the demo lid the floor of
    /// the transition would tilt at about 15° where the lid itself is 6°.
    /// Lengthening the short ramps (never shortening a Jacobian ramp) pulls
    /// neighbouring floors together at `tan(cap)`. A column with no lift is
    /// still the identity however long its ramp is, so the rim can take a long
    /// blend without moving.
    pub fn smooth_transition_ramps(&mut self) {
        self.smooth_transition_ramps_with(0.98);
    }

    /// As [`Self::smooth_transition_ramps`], but the floor may tilt by only
    /// `margin` times `tan(cap)`.
    ///
    /// The floor itself is not what the toolhead follows. A level in the middle
    /// of the blend is steeper than the floor, so the caller tightens `margin`
    /// until those levels fit the cap. Lengthening is still the only change:
    /// a ramp never goes below its Jacobian minimum.
    pub fn smooth_transition_ramps_with(&mut self, margin: f64) {
        let width = self.grid.width;
        let height = self.grid.height;
        let n = width * height;
        let step = self.grid.step.max(1e-9);
        let max_dz = self.max_angle_deg.to_radians().tan() * step * margin.clamp(0.05, 1.0);
        let floor_z = self.transition_floor_z();
        let mut present = vec![false; n];
        let mut start = vec![0.0; n];
        let mut ramp_min = vec![self.ramp_thickness.max(1e-6); n];
        for i in 0..n {
            let Some(z_band) = self.grid.z_band_start.get(i).copied().flatten() else {
                continue;
            };
            if self.grid.z_top_smooth.get(i).is_none_or(|z| z.is_none()) {
                continue;
            }
            let min_ramp = self.transition_thickness_for_lift(self.node_lift(i));
            ramp_min[i] = min_ramp;
            // Highest ramp start that still meets the Jacobian.
            start[i] = z_band - min_ramp;
            present[i] = true;
        }
        if max_dz > 1e-9 {
            let diag = max_dz * std::f64::consts::SQRT_2;
            for iy in 0..height {
                for ix in 0..width {
                    let i = iy * width + ix;
                    if !present[i] {
                        continue;
                    }
                    if ix > 0 && present[i - 1] {
                        start[i] = start[i].min(start[i - 1] + max_dz);
                    }
                    if iy > 0 && present[i - width] {
                        start[i] = start[i].min(start[i - width] + max_dz);
                    }
                    if ix > 0 && iy > 0 && present[i - width - 1] {
                        start[i] = start[i].min(start[i - width - 1] + diag);
                    }
                    if ix + 1 < width && iy > 0 && present[i - width + 1] {
                        start[i] = start[i].min(start[i - width + 1] + diag);
                    }
                }
            }
            for iy in (0..height).rev() {
                for ix in (0..width).rev() {
                    let i = iy * width + ix;
                    if !present[i] {
                        continue;
                    }
                    if ix + 1 < width && present[i + 1] {
                        start[i] = start[i].min(start[i + 1] + max_dz);
                    }
                    if iy + 1 < height && present[i + width] {
                        start[i] = start[i].min(start[i + width] + max_dz);
                    }
                    if ix + 1 < width && iy + 1 < height && present[i + width + 1] {
                        start[i] = start[i].min(start[i + width + 1] + diag);
                    }
                    if ix > 0 && iy + 1 < height && present[i + width - 1] {
                        start[i] = start[i].min(start[i + width - 1] + diag);
                    }
                }
            }
        }
        let mut ramps = vec![0.0; n];
        for i in 0..n {
            if !present[i] {
                continue;
            }
            let z_band = self.grid.z_band_start[i].unwrap_or(0.0);
            let smoothed = start[i].max(floor_z);
            ramps[i] = (z_band - smoothed).max(ramp_min[i]);
        }
        self.node_ramp = ramps;
    }

    /// Point each printable island's flat target at its footprint rim.
    ///
    /// The rim is the lowest top on the boundary of the columns that still
    /// lift. Lift is then `flat - h_top`: zero on that rim and negative toward
    /// a peak inside it, so a gentle dome is pressed down onto the rim instead
    /// of the rim being pulled up to the peak. Planned underside bands keep
    /// the flat the plan stamped into them.
    ///
    /// Only a real dome is retargeted: the highest column has to sit inside
    /// the footprint, above every boundary column, by at least one layer.
    /// A blur lip is shallower than that, and a descending slope has its high
    /// end on the boundary. Pressing either down leaves Cura's top sheet above
    /// the model. They keep the peak target [`assign_island_flat_targets`] wrote.
    pub fn retarget_flats_to_footprint_rim(&mut self) {
        let width = self.grid.width;
        let height = self.grid.height;
        let n = width * height;
        let mut member = vec![false; n];
        for (i, is_member) in member.iter_mut().enumerate() {
            if self.grid.z_deform_cap.get(i).is_some_and(|z| z.is_some()) {
                continue;
            }
            if self.island_strength.get(i).copied().unwrap_or(0.0) <= 0.0 {
                continue;
            }
            if self.grid.z_top.get(i).is_none_or(|z| z.is_none()) {
                continue;
            }
            if self.grid.z_top_smooth.get(i).is_none_or(|z| z.is_none()) {
                continue;
            }
            *is_member = true;
        }
        let mut labels = vec![0u32; n];
        let mut next = 0u32;
        let mut stack: Vec<usize> = Vec::new();
        let mut rims: Vec<f64> = vec![0.0];
        let mut dome: Vec<bool> = vec![false];
        let layer = self.nominal_layer.max(0.05);
        for start in 0..n {
            if !member[start] || labels[start] != 0 {
                continue;
            }
            next += 1;
            labels[start] = next;
            stack.push(start);
            let mut boundary_min = f64::INFINITY;
            let mut boundary_max = f64::NEG_INFINITY;
            let mut interior_max = f64::NEG_INFINITY;
            let mut component_min = f64::INFINITY;
            let mut saw_boundary = false;
            while let Some(i) = stack.pop() {
                let z = self.grid.z_top_smooth[i].unwrap_or(0.0);
                component_min = component_min.min(z);
                let ix = i % width;
                let iy = i / width;
                let mut boundary = ix == 0 || iy == 0 || ix + 1 == width || iy + 1 == height;
                let neighbours = [
                    (ix > 0).then(|| i - 1),
                    (ix + 1 < width).then(|| i + 1),
                    (iy > 0).then(|| i - width),
                    (iy + 1 < height).then(|| i + width),
                ];
                for neighbour in neighbours.into_iter().flatten() {
                    if member[neighbour] {
                        if labels[neighbour] == 0 {
                            labels[neighbour] = next;
                            stack.push(neighbour);
                        }
                    } else {
                        boundary = true;
                    }
                }
                if boundary {
                    saw_boundary = true;
                    boundary_min = boundary_min.min(z);
                    boundary_max = boundary_max.max(z);
                } else {
                    interior_max = interior_max.max(z);
                }
            }
            let rim = if saw_boundary && boundary_min.is_finite() {
                boundary_min
            } else {
                component_min
            };
            let is_dome = interior_max.is_finite()
                && boundary_max.is_finite()
                && interior_max > boundary_max + 1e-3
                && interior_max - rim >= layer;
            rims.push(rim);
            dome.push(is_dome);
        }
        let mut highest = f64::NEG_INFINITY;
        for (i, label) in labels.iter().enumerate() {
            if *label == 0 || !dome[*label as usize] {
                continue;
            }
            let rim = rims[*label as usize];
            self.grid.z_deform_flat[i] = Some(rim);
            highest = highest.max(rim);
        }
        if highest.is_finite() {
            self.deformed_top_z = highest;
        }
    }
}

pub fn build_deform_field(
    grid: HeightGrid,
    band_thickness: f64,
    ramp_thickness: f64,
    nominal_layer: f64,
    max_angle_deg: f64,
) -> DeformField {
    let mut grid = grid;
    let top_islands = assign_island_flat_targets(&mut grid);
    let mut max_top = 0.0f64;
    for v in grid.z_top_smooth.iter().flatten() {
        max_top = max_top.max(*v);
    }
    let deformed_top_z = max_top;
    let min_layer = 0.08;
    let max_layer = 0.28;
    let len = grid.width * grid.height;
    DeformField {
        grid,
        band_thickness,
        ramp_thickness,
        deformed_top_z,
        min_layer,
        max_layer,
        nominal_layer,
        island_strength: vec![1.0; len],
        max_angle_deg,
        top_islands,
        node_ramp: Vec::new(),
    }
}

/// Give every connected top island its own flat target.
///
/// `forward_z` lifts a column's top to `z_deform_flat`. With a single global
/// target, a plate beside a tall tower would be lifted to the tower's height and
/// its band squashed into a fraction of a millimetre of plastic. Islands are the
/// connected regions of the top grid — exactly the columns a slicer treats as one
/// top sheet — so each sheet flattens to its own highest point.
///
/// Columns owned by a planned underside band (`z_deform_cap`) keep the flat
/// target the plan stamped into them.
pub fn assign_island_flat_targets(grid: &mut HeightGrid) -> usize {
    let width = grid.width;
    let height = grid.height;
    let mut labels: Vec<u32> = vec![0; width * height];
    let mut island_max: Vec<f64> = vec![0.0];
    let mut next = 0u32;
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..width * height {
        if labels[start] != 0 || grid.z_top_smooth[start].is_none() {
            continue;
        }
        next += 1;
        let mut max_z = f64::NEG_INFINITY;
        labels[start] = next;
        stack.push(start);
        while let Some(i) = stack.pop() {
            if let Some(z) = grid.z_top_smooth[i] {
                max_z = max_z.max(z);
            }
            let ix = i % width;
            let iy = i / width;
            let neighbours = [
                (ix > 0).then(|| i - 1),
                (ix + 1 < width).then(|| i + 1),
                (iy > 0).then(|| i - width),
                (iy + 1 < height).then(|| i + width),
            ];
            for neighbour in neighbours.into_iter().flatten() {
                if labels[neighbour] == 0 && grid.z_top_smooth[neighbour].is_some() {
                    labels[neighbour] = next;
                    stack.push(neighbour);
                }
            }
        }
        island_max.push(max_z);
    }
    for (i, label) in labels.iter().enumerate() {
        if *label == 0 || grid.z_deform_cap[i].is_some() {
            continue;
        }
        grid.z_deform_flat[i] = Some(island_max[*label as usize]);
    }
    next as usize
}

/// Deform mesh vertices in place (copy returned).
pub fn deform_mesh(mesh: &Mesh, field: &DeformField) -> Mesh {
    let mut out = mesh.clone();
    for vertex in &mut out.vertices {
        let [x, y, z] = *vertex;
        // The ramp below the band moves too. Skipping it left a step of the full
        // lift at `z_band`: fine when the limiter had already eaten the lift,
        // a self-intersection once the rim-referenced peak is pressed down by
        // a few millimetres. `forward_z` is the identity below the ramp start.
        vertex[2] = field.forward_z(x, y, z);
    }
    out
}

/// Subdivide long edges so Cura sees ≤ `max_edge_mm` segments (midpoint split, one pass).
pub fn subdivide_long_edges(mesh: &Mesh, max_edge_mm: f64) -> Mesh {
    let max_edge = max_edge_mm.max(0.2);
    let mut vertices = mesh.vertices.clone();
    let mut triangles = mesh.triangles.clone();
    let mut split_edge: std::collections::HashMap<(u32, u32), u32> =
        std::collections::HashMap::new();

    let mut need_split = true;
    let mut pass = 0;
    while need_split && pass < 4 {
        need_split = false;
        pass += 1;
        let mut new_tris = Vec::new();
        for tri in triangles {
            let mut edges = [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])];
            edges.sort_by_key(|&(a, b)| {
                let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                (lo, hi)
            });
            let mut mid = [None; 3];
            for (i, &(a, b)) in edges.iter().enumerate() {
                let va = vertices[a as usize];
                let vb = vertices[b as usize];
                let len =
                    ((va[0] - vb[0]).powi(2) + (va[1] - vb[1]).powi(2) + (va[2] - vb[2]).powi(2))
                        .sqrt();
                if len > max_edge {
                    need_split = true;
                    let key = (a.min(b), a.max(b));
                    let index = *split_edge.entry(key).or_insert_with(|| {
                        let m = [
                            0.5 * (va[0] + vb[0]),
                            0.5 * (va[1] + vb[1]),
                            0.5 * (va[2] + vb[2]),
                        ];
                        vertices.push(m);
                        (vertices.len() - 1) as u32
                    });
                    mid[i] = Some(index);
                }
            }
            match (mid[0], mid[1], mid[2]) {
                (None, None, None) => new_tris.push(tri),
                (Some(m01), None, None) => {
                    new_tris.push([tri[0], m01, tri[2]]);
                    new_tris.push([m01, tri[1], tri[2]]);
                }
                (None, Some(m12), None) => {
                    new_tris.push([tri[0], tri[1], m12]);
                    new_tris.push([tri[0], m12, tri[2]]);
                }
                (None, None, Some(m20)) => {
                    new_tris.push([tri[0], tri[1], m20]);
                    new_tris.push([tri[1], tri[2], m20]);
                }
                _ => new_tris.push(tri), // rare triple split: defer to next pass
            }
        }
        triangles = new_tris;
    }
    Mesh {
        vertices,
        triangles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::height_field::build_height_field;
    use crate::test_meshes::sphere_mesh;

    #[test]
    fn forward_map_is_monotone_in_z() {
        let mesh = sphere_mesh(10.0);
        let grid = build_height_field(&mesh, 1.0, 45.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 45.0);
        assert!(field.verify_monotone(20), "dh/dz must stay positive");
    }

    /// Uniform 3x3 grid whose every column has the same top, lift and band.
    fn synthetic_field(lift: f64, z_band: f64, ramp: f64) -> DeformField {
        let band = 1.6;
        let h_top = z_band + band;
        let flat = h_top + lift;
        let n = 3usize;
        let len = n * n;
        DeformField {
            grid: HeightGrid {
                origin: [0.0, 0.0],
                step: 1.0,
                width: n,
                height: n,
                z_top: vec![Some(h_top); len],
                z_top_smooth: vec![Some(h_top); len],
                z_band_start: vec![Some(z_band); len],
                z_deform_cap: vec![None; len],
                z_deform_flat: vec![Some(flat); len],
            },
            band_thickness: band,
            ramp_thickness: ramp,
            deformed_top_z: flat,
            min_layer: 0.08,
            max_layer: 0.28,
            nominal_layer: 0.2,
            island_strength: vec![1.0; len],
            max_angle_deg: 10.0,
            top_islands: 1,
            node_ramp: Vec::new(),
        }
    }

    /// Worst slack of `r = dz_phys / dz_def` against the Jacobian window.
    fn jacobian_scan(field: &DeformField, x: f64, y: f64, z_top: f64) -> (f64, f64) {
        let (r_min, r_max) = field.jacobian_bounds();
        let dz = field.nominal_layer;
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let steps = (z_top / 0.01).ceil() as usize;
        for i in 0..=steps {
            let z = z_top * i as f64 / steps as f64;
            let dz_def = field.forward_z(x, y, z + dz) - field.forward_z(x, y, z);
            let r = dz / dz_def.max(1e-12);
            lo = lo.min(r);
            hi = hi.max(r);
        }
        (lo - r_min, r_max - hi)
    }

    #[test]
    fn transition_ramp_is_sized_from_the_lift() {
        let field = synthetic_field(4.0, 12.0, 0.4);
        let (r_min, r_max) = field.jacobian_bounds();
        assert!((r_min - 0.4).abs() < 1e-9 && (r_max - 1.4).abs() < 1e-9);
        let expected = 4.0 * r_min / (1.0 - r_min);
        let ramp = field.transition_thickness_for_lift(4.0);
        assert!(
            (ramp - expected).abs() < 1e-9,
            "lift 4 needs {expected} mm of transition, got {ramp}"
        );
        let t = field.column_transition(0.5, 0.5).expect("column");
        assert!((t.ramp - expected).abs() < 1e-9);
        assert!((t.z_band - t.ramp - (12.0 - expected)).abs() < 1e-9);
    }

    #[test]
    fn positive_lift_stays_inside_the_jacobian_bounds() {
        let field = synthetic_field(8.4, 14.0, 0.4);
        let (lo_slack, hi_slack) = jacobian_scan(&field, 0.5, 0.5, 14.0 + 1.6);
        assert!(
            lo_slack >= -1e-9,
            "physical layer thickness fell {lo_slack} below min_layer"
        );
        assert!(
            hi_slack >= -1e-9,
            "physical layer thickness rose {hi_slack} above max_layer"
        );
        let t = field.column_transition(0.5, 0.5).expect("column");
        let mid = t.z_band - 0.5 * t.ramp;
        let dz = 0.01;
        let r = dz / (field.forward_z(0.5, 0.5, mid + dz) - field.forward_z(0.5, 0.5, mid));
        assert!(
            (r - (0.08 / 0.2)).abs() < 1e-6,
            "r must sit exactly on r_min in the transition, got {r}"
        );
    }

    #[test]
    fn negative_lift_stretches_within_the_jacobian_bounds() {
        let field = synthetic_field(-1.0, 12.0, 0.4);
        let (_, r_max) = field.jacobian_bounds();
        let expected = 1.0 * r_max / (r_max - 1.0);
        let ramp = field.transition_thickness_for_lift(-1.0);
        assert!(
            (ramp - expected).abs() < 1e-9,
            "stretch 1 mm needs {expected} mm, got {ramp}"
        );
        let (lo_slack, hi_slack) = jacobian_scan(&field, 0.5, 0.5, 13.6);
        assert!(lo_slack >= -1e-9 && hi_slack >= -1e-9);
        assert!(ramp > 1.0, "ramp {ramp} must exceed the 1 mm stretch");
    }

    #[test]
    fn forward_and_inverse_are_exact_inverses() {
        for lift in [8.4, 1.0, 0.05, -0.7] {
            let field = synthetic_field(lift, 12.0, 0.4);
            let t = field.column_transition(0.5, 0.5).expect("column");
            let top = 13.6;
            for i in 0..=400 {
                let z = top * i as f64 / 400.0;
                let f = field.forward_z(0.5, 0.5, z);
                let back = field.inverse_z(0.5, 0.5, f);
                assert!(
                    (back - z).abs() < 1e-9,
                    "lift {lift}: z {z} round-tripped to {back} (band {} ramp {})",
                    t.z_band,
                    t.ramp
                );
            }
        }
    }

    #[test]
    fn short_columns_attenuate_instead_of_crushing() {
        let field = synthetic_field(8.0, 1.0, 0.4);
        let t = field.column_transition(0.5, 0.5).expect("column");
        assert!(t.lift > 0.0 && t.lift < 8.0, "strength must attenuate");
        let (lo_slack, hi_slack) = jacobian_scan(&field, 0.5, 0.5, 2.6);
        assert!(lo_slack >= -1e-9, "attenuated field still crushes");
        assert!(hi_slack >= -1e-9);
        assert!(
            t.z_band - t.ramp >= field.transition_floor_z() - 1e-9,
            "ramp starts at {:.3}, floor is {:.3}",
            t.z_band - t.ramp,
            field.transition_floor_z()
        );
    }

    #[test]
    fn no_room_below_the_band_leaves_the_column_planar() {
        let field = synthetic_field(8.0, 0.3, 0.4);
        let t = field.column_transition(0.5, 0.5).unwrap();
        assert!(t.lift.abs() < 1e-12 || field.forward_z(0.5, 0.5, 0.2) == 0.2);
        // Attenuation is applied by the explicit pass; the on-the-fly map still
        // sizes the ramp. A column with no room is zeroed by attenuate_transitions.
        let mut field = field;
        assert!(field.attenuate_transitions() > 0);
        assert!(field.node_lift(0).abs() < 1e-9);
        for z in [0.0, 0.1, 0.3, 1.0] {
            assert!((field.forward_z(0.5, 0.5, z) - z).abs() < 1e-12);
        }
        assert!(field.local_band_floor_z(0.5, 0.5).is_none());
        assert!(!field.gcode_needs_inverse(0.5, 0.5, 0.5));
    }

    #[test]
    fn engaged_columns_ignores_extrapolated_padding() {
        let mut field = synthetic_field(1.0, 12.0, 0.4);
        for top in field.grid.z_top.iter_mut().take(9).skip(2) {
            *top = None;
        }
        assert_eq!(field.engaged_columns(), 2);
        assert_eq!(field.material_columns(), 2);
        assert_eq!(field.island_strength.len(), 9);
    }

    #[test]
    fn attenuating_transitions_reports_the_columns_it_touched() {
        let mut field = synthetic_field(8.0, 1.0, 0.4);
        assert_eq!(field.attenuate_transitions(), 9);
        assert_eq!(
            field.attenuate_transitions(),
            0,
            "attenuation must be idempotent"
        );
    }
}
