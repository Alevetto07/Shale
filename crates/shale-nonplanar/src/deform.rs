//! Mesh deformation for the curved-top band (ramp + band, monotone in Z).

use crate::mesh::Mesh;

use crate::height_field::HeightGrid;

/// Maps physical coordinates to the deformed space used for planar slicing.
#[derive(Clone, Debug)]
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
}

impl DeformField {
    /// Conservative Z cut: below this height, deformation should not apply.
    pub fn global_band_floor_z(&self) -> f64 {
        let max_top = self
            .grid
            .z_top_smooth
            .iter()
            .filter_map(|z| *z)
            .fold(0.0_f64, f64::max);
        max_top - self.band_thickness - self.ramp_thickness
    }

    fn column_limits(&self, x: f64, y: f64) -> Option<(f64, f64, f64, f64)> {
        let z_band = self.grid.sample_band_start(x, y)?;
        let h_top = self
            .grid
            .sample_top_smooth(x, y)
            .unwrap_or(self.deformed_top_z);
        let strength = self.island_strength_at(x, y);
        Some((z_band, h_top, strength, self.deformed_top_z))
    }

    /// True when this G-code point lies in the curved-top shell (deformed coordinates).
    pub fn gcode_needs_inverse(&self, x: f64, y: f64, z_def: f64) -> bool {
        let Some((z_band, _, strength, _)) = self.column_limits(x, y) else {
            return false;
        };
        if strength <= 0.0 {
            return false;
        }
        let z_phys = self.inverse_z(x, y, z_def);
        z_phys > z_band + 1e-9 && (z_phys - z_def).abs() > 5e-3
    }

    /// Forward map: physical Z → deformed Z. Linear and monotone within the top band.
    pub fn forward_z(&self, x: f64, y: f64, z: f64) -> f64 {
        let Some((z_band, h_top, strength, flat_top)) = self.column_limits(x, y) else {
            return z;
        };
        if strength <= 0.0 || z <= z_band {
            return z;
        }
        let wall_shift = (flat_top - h_top) * strength;
        if z >= h_top {
            return z + wall_shift;
        }
        let span = (h_top - z_band).max(1e-6);
        let target = z_band + (z - z_band) / span * (flat_top - z_band);
        z + strength * (target - z)
    }

    /// Effective layer thickness ratio for extrusion correction.
    pub fn layer_thickness_ratio(&self, x: f64, y: f64, z0: f64, z1: f64) -> f64 {
        let dz_phys = (z1 - z0).abs().max(1e-6);
        let d0 = self.forward_z(x, y, z0);
        let d1 = self.forward_z(x, y, z1);
        let dz_def = (d1 - d0).abs();
        (dz_def / dz_phys).clamp(self.min_layer / self.nominal_layer, self.max_layer / self.nominal_layer)
    }

    pub fn island_strength_at(&self, x: f64, y: f64) -> f64 {
        if self.island_strength.len() == 1 {
            return self.island_strength[0];
        }
        let fx = (x - self.grid.origin[0]) / self.grid.step;
        let fy = (y - self.grid.origin[1]) / self.grid.step;
        let ix = fx.floor().clamp(0.0, self.grid.width as f64 - 1.0) as usize;
        let iy = fy.floor().clamp(0.0, self.grid.height as f64 - 1.0) as usize;
        self.island_strength
            .get(iy * self.grid.width + ix)
            .copied()
            .unwrap_or(1.0)
    }

    /// Inverse Z: algebraic inverse of the linear `forward_z`.
    pub fn inverse_z(&self, x: f64, y: f64, z_deformed: f64) -> f64 {
        let Some((z_band, h_top, strength, flat_top)) = self.column_limits(x, y) else {
            return z_deformed;
        };
        if strength <= 0.0 || z_deformed <= z_band + 1e-12 {
            return z_deformed;
        }
        let wall_shift = (flat_top - h_top) * strength;
        let f_top = h_top + wall_shift;
        if z_deformed >= f_top - 1e-12 {
            return z_deformed - wall_shift;
        }
        let span = (h_top - z_band).max(1e-6);
        let a = 1.0 - strength + strength * (flat_top - z_band) / span;
        let b = strength * (z_band - (flat_top - z_band) / span * z_band);
        if a.abs() < 1e-12 {
            return z_deformed;
        }
        (z_deformed - b) / a
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
}

pub fn build_deform_field(
    grid: HeightGrid,
    band_thickness: f64,
    ramp_thickness: f64,
    nominal_layer: f64,
    max_angle_deg: f64,
) -> DeformField {
    let mut max_top = 0.0f64;
    for z in &grid.z_top_smooth {
        if let Some(v) = z {
            max_top = max_top.max(*v);
        }
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
    }
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
    let mut split_edge: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();

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
                let len = ((va[0] - vb[0]).powi(2) + (va[1] - vb[1]).powi(2) + (va[2] - vb[2]).powi(2)).sqrt();
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
    Mesh { vertices, triangles }
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

    #[test]
    fn layer_thickness_ratio_is_clamped() {
        let mesh = crate::test_meshes::box_mesh(10.0, 10.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let field = build_deform_field(grid, 2.0, 0.4, 0.2, 30.0);
        let r = field.layer_thickness_ratio(0.0, 0.0, 8.0, 9.0);
        assert!(r >= 0.08 / 0.2 - 0.01);
        assert!(r <= 0.28 / 0.2 + 0.01);
    }
}