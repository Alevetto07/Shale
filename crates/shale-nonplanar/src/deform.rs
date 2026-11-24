//! Mesh deformation for the curved-top band (ramp + band, monotone in Z).

use crate::mesh::Mesh;
use serde::{Deserialize, Serialize};

use crate::height_field::HeightGrid;

/// How one column maps: the shear lift and the physical transition below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    /// Physical Z where the column's band starts (the shear applies above this).
    pub z_band: f64,
    /// Vertical displacement applied above `z_band` (negative = stretched down).
    pub lift: f64,
    /// Physical thickness of the blend between the bed and `z_band`.
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
        for (i, top) in self.grid.z_top_smooth.iter().enumerate() {
            if top.is_none() {
                continue;
            }
            if let Some(z) = self.grid.z_band_start[i] {
                floor = floor.min(z);
            }
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

    pub(crate) fn column_limits(&self, x: f64, y: f64) -> Option<(f64, f64, f64, f64)> {
        let z_band = self.grid.sample_band_start(x, y)?;
        let h_top = self
            .grid
            .sample_top_smooth(x, y)
            .unwrap_or(self.deformed_top_z);
        let strength = self.island_strength_at(x, y);
        let flat_top = self
            .grid
            .sample_deform_flat(x, y)
            .unwrap_or(self.deformed_top_z);
        Some((z_band, h_top, strength, flat_top))
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
    /// strength that fits below `z_band` has a closed form; taking the smaller of
    /// it and the planned strength keeps the schedule continuous (both terms are
    /// continuous in `z_band` and in the lift, and the cap goes to zero exactly as
    /// the room does).
    fn fitted_strength(&self, z_band: f64, base: f64, strength: f64) -> f64 {
        let strength = strength.clamp(0.0, 1.0);
        if strength <= 0.0 || base.abs() <= 1e-12 {
            return strength;
        }
        let available = (z_band - self.transition_floor_z()).max(0.0);
        let configured = self.ramp_thickness.max(1e-6);
        if available <= configured {
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

    /// Lift and physical transition thickness this column actually applies.
    ///
    /// The single source of truth for the forward map, the inverse map and the
    /// G-code side: every one of them has to read the same attenuated strength and
    /// the same ramp, or the round trip stops being exact.
    pub fn column_transition(&self, x: f64, y: f64) -> Option<Transition> {
        let (z_band, h_top, strength, flat_top) = self.column_limits(x, y)?;
        let base = flat_top - h_top;
        let strength = self.fitted_strength(z_band, base, strength);
        let lift = strength * base;
        Some(Transition {
            z_band,
            lift,
            ramp: self.transition_thickness_for_lift(lift),
        })
    }

    /// Lowest deformed Z at this XY: below it the map is the identity.
    ///
    /// The G-code side used to compare against [`Self::global_band_floor_z`],
    /// the lowest band floor anywhere on the plate. That passes through any
    /// move below the global floor even when the *local* band starts well
    /// above it, so a lifted island next to a lower one printed its upper band
    /// at un-mapped Z and the next layer descended into it. Every skip test
    /// has to be local to the column it is judged in.
    pub fn local_band_floor_z(&self, x: f64, y: f64) -> Option<f64> {
        self.column_transition(x, y).and_then(|t| {
            (t.lift.abs() > 1e-12).then_some(t.z_band - t.ramp)
        })
    }

    /// True when this G-code point lies in the curved-top shell (deformed coordinates).
    pub fn gcode_needs_inverse(&self, x: f64, y: f64, z_def: f64) -> bool {
        let Some(t) = self.column_transition(x, y) else {
            return false;
        };
        if t.lift.abs() < 1e-12 {
            return false;
        }
        // The ramp below the band is mapped too, at a fraction of the lift.
        let z_phys = self.inverse_z(x, y, z_def);
        z_phys > t.z_band - t.ramp + 1e-9 && (z_phys - z_def).abs() > 5e-3
    }

    /// Forward map: physical Z → deformed Z.
    ///
    /// Above the band floor the map is a pure vertical **shear**: the whole
    /// cross-section rises by the column's lift `strength * (flat - h_top)`, so
    /// the deformed top is flat while every level surface inside the band keeps
    /// the model's own slope. Below it, a per-column ramp blends the lift to
    /// zero, so the part under the band is untouched and the map stays monotone
    /// in Z.
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
    ///
    /// The ramp is as long as the lift needs it to be: the map is linear through
    /// it, so `r = ramp / (ramp + lift)` throughout, and
    /// [`Self::transition_thickness_for_lift`] sizes the ramp so `r` never leaves
    /// `[min_layer, max_layer] / nominal`. With a fixed ramp the old shear put
    /// `lift + ramp` of deformed Z into `ramp` of plastic — for the demo lid's
    /// 8.4 mm lift, 0.4 mm of physical Z carrying 22 deformed layers, a physical
    /// layer thickness of 0.018 mm. The column's strength is attenuated where the
    /// part below `z_band` has no room for that ramp (see [`Self::fitted_strength`]).
    pub fn forward_z(&self, x: f64, y: f64, z: f64) -> f64 {
        let Some(t) = self.column_transition(x, y) else {
            return z;
        };
        if self
            .grid
            .sample_deform_cap(x, y)
            .is_some_and(|cap| z > cap + 1e-9)
        {
            return z;
        }
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

    /// Local `dz_def / dz_phys` (the inverse of the physical thickness ratio).
    ///
    /// Deliberately unclamped: a ratio outside `[min_layer, max_layer] /
    /// nominal` is a field that crushes or stretches the part, and the safety
    /// pass has to see the real number to reject it. Clamping here was hiding a
    /// several-fold over-extrusion in the transition.
    pub fn layer_thickness_ratio(&self, x: f64, y: f64, z0: f64, z1: f64) -> f64 {
        let dz_phys = (z1 - z0).abs().max(1e-6);
        let d0 = self.forward_z(x, y, z0);
        let d1 = self.forward_z(x, y, z1);
        let dz_def = (d1 - d0).abs();
        dz_def / dz_phys
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
            return self
                .island_strength
                .last()
                .copied()
                .unwrap_or(1.0)
                .min(1.0);
        };
        let w = self.grid.width;
        let s = &self.island_strength;
        let value = s[iy * w + ix] * (1.0 - tx) * (1.0 - ty)
            + s[iy * w + ix + 1] * tx * (1.0 - ty)
            + s[(iy + 1) * w + ix] * (1.0 - tx) * ty
            + s[(iy + 1) * w + ix + 1] * tx * ty;
        value.clamp(0.0, 1.0)
    }

    /// Inverse Z: algebraic inverse of the shear in [`Self::forward_z`].
    pub fn inverse_z(&self, x: f64, y: f64, z_deformed: f64) -> f64 {
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
    pub fn min_strength(&self) -> f64 {
        let mut min = f64::INFINITY;
        for (i, strength) in self.island_strength.iter().enumerate() {
            if self.grid.z_top.get(i).is_some_and(|z| z.is_some()) {
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
            if self.grid.z_top.get(i).is_some_and(|z| z.is_some()) {
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
    ///
    /// Counts real material columns only. The smoothed top is extrapolated into
    /// the pad around the part, so counting `z_top_smooth` reported more
    /// "footprint" columns than the plate has printed columns (34078 of 33990 on
    /// the demo lid) and made every ratio against the part's own column count
    /// nonsense.
    pub fn engaged_columns(&self) -> usize {
        self.island_strength
            .iter()
            .enumerate()
            .filter(|(i, s)| **s > 0.0 && self.grid.z_top[*i].is_some())
            .count()
    }

    /// Columns with a printable top of their own (the real footprint size).
    pub fn material_columns(&self) -> usize {
        self.grid.z_top.iter().filter(|z| z.is_some()).count()
    }

    /// Lower every column's strength so its transition fits above the bed.
    ///
    /// Runs before the slope limiter: the transition is linear in the ramp, so
    /// the strength that fits below `z_band` has a closed form
    /// ([`Self::fitted_strength`]) and the map itself applies the same cap at
    /// every sample, including between grid nodes. Returns the number of columns
    /// whose strength had to drop.
    pub fn attenuate_transitions(&mut self) -> usize {
        let mut changed = 0usize;
        for i in 0..self.island_strength.len() {
            let (Some(z_band), Some(h_top)) = (self.grid.z_band_start[i], self.grid.z_top_smooth[i])
            else {
                continue;
            };
            let flat = self.grid.z_deform_flat[i].unwrap_or(self.deformed_top_z);
            let strength = self.island_strength[i];
            let fitted = self.fitted_strength(z_band, flat - h_top, strength);
            if fitted < strength - 1e-12 {
                self.island_strength[i] = fitted;
                changed += 1;
            }
        }
        changed
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
        let below_band = field
            .grid
            .sample_band_start(x, y)
            .is_none_or(|z_band| z <= z_band + 1e-9);
        if below_band {
            continue;
        }
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

    /// Uniform 3x3 grid whose every column has the same top, lift and band.
    ///
    /// A synthetic grid rather than a mesh: the whole point of these tests is a
    /// precise lift, and a mesh+smoothing pipeline cannot produce one.
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
        }
    }

    /// Worst `r = dz_phys / dz_def` and the worst deviation outside the bounds.
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
    fn forward_map_is_monotone_in_z() {
        let mesh = sphere_mesh(10.0);
        let grid = build_height_field(&mesh, 1.0, 45.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 45.0);
        assert!(field.verify_monotone(20), "dh/dz must stay positive");
    }

    #[test]
    fn transition_ramp_is_sized_from_the_lift() {
        let field = synthetic_field(4.0, 12.0, 0.4);
        let (r_min, r_max) = field.jacobian_bounds();
        assert!((r_min - 0.4).abs() < 1e-9 && (r_max - 1.4).abs() < 1e-9);
        // r = ramp / (ramp + lift) >= r_min  <=>  ramp >= lift * r_min / (1 - r_min).
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
        // The transition is the only place the ratio moves; the band is a shear.
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
        // A stretched column needs a transition longer than the stretch itself,
        // or the map would not stay monotone.
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
        // 1.0 mm of part under the band: only 0.6 mm above the two-layer floor.
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
        // Attenuation is continuous in the room below the band: no cliff between
        // a column with 0.6 mm and one with 0.65 mm of transition space.
        let a = synthetic_field(8.0, 1.0, 0.4)
            .column_transition(0.5, 0.5)
            .unwrap()
            .lift;
        let b = synthetic_field(8.0, 1.05, 0.4)
            .column_transition(0.5, 0.5)
            .unwrap()
            .lift;
        assert!(b > a && b - a < 0.2, "attenuation jumped from {a} to {b}");
    }

    #[test]
    fn no_room_below_the_band_leaves_the_column_planar() {
        let field = synthetic_field(8.0, 0.3, 0.4);
        assert!(field.column_transition(0.5, 0.5).is_none() || {
            let t = field.column_transition(0.5, 0.5).unwrap();
            t.lift.abs() < 1e-12
        });
        for z in [0.0, 0.1, 0.3, 1.0] {
            assert!((field.forward_z(0.5, 0.5, z) - z).abs() < 1e-12);
        }
        assert!(field.local_band_floor_z(0.5, 0.5).is_none());
        assert!(!field.gcode_needs_inverse(0.5, 0.5, 0.5));
    }

    #[test]
    fn engaged_columns_ignores_extrapolated_padding() {
        let mut field = synthetic_field(1.0, 12.0, 0.4);
        // Two of nine columns are real material; the blur filled the rest in.
        for i in 2..9 {
            field.grid.z_top[i] = None;
        }
        assert_eq!(field.engaged_columns(), 2);
        assert_eq!(field.material_columns(), 2);
        assert_eq!(field.island_strength.len(), 9);
    }

    #[test]
    fn attenuating_transitions_reports_the_columns_it_touched() {
        let mut field = synthetic_field(8.0, 1.0, 0.4);
        assert_eq!(field.attenuate_transitions(), 9);
        let again = field.attenuate_transitions();
        assert_eq!(again, 0, "attenuation must be idempotent");
    }
}
