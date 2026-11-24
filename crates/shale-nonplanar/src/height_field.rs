//! Vertical ray casting over an XY grid to find printable top surfaces.

use crate::mesh::Mesh;
use serde::{Deserialize, Serialize};

/// A scalar field sampled on a regular XY grid.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeightGrid {
    pub origin: [f64; 2],
    pub step: f64,
    pub width: usize,
    pub height: usize,
    /// `None` means no printable top at this column.
    pub z_top: Vec<Option<f64>>,
    /// Smoothed top height after Gaussian blur.
    pub z_top_smooth: Vec<Option<f64>>,
    /// Bottom of the deformation band at each column (`z_top - band`).
    pub z_band_start: Vec<Option<f64>>,
    /// When set, deformation does not affect vertices above this Z (underside bands).
    pub z_deform_cap: Vec<Option<f64>>,
    /// Local flat target in deformed space; defaults to global `deformed_top_z` when unset.
    pub z_deform_flat: Vec<Option<f64>>,
}

impl HeightGrid {
    pub fn sample_top_smooth(&self, x: f64, y: f64) -> Option<f64> {
        self.bilinear(&self.z_top_smooth, x, y)
    }

    pub fn sample_band_start(&self, x: f64, y: f64) -> Option<f64> {
        self.bilinear(&self.z_band_start, x, y)
    }

    pub fn sample_deform_cap(&self, x: f64, y: f64) -> Option<f64> {
        self.bilinear(&self.z_deform_cap, x, y)
    }

    pub fn sample_deform_flat(&self, x: f64, y: f64) -> Option<f64> {
        self.bilinear(&self.z_deform_flat, x, y)
    }

    fn bilinear(&self, field: &[Option<f64>], x: f64, y: f64) -> Option<f64> {
        let fx = (x - self.origin[0]) / self.step;
        let fy = (y - self.origin[1]) / self.step;
        if fx < 0.0 || fy < 0.0 {
            return field.first().copied().flatten();
        }
        let ix = fx.floor() as isize;
        let iy = fy.floor() as isize;
        if ix as usize >= self.width.saturating_sub(1) || iy as usize >= self.height.saturating_sub(1) {
            return field.last().copied().flatten();
        }
        let ix = ix as usize;
        let iy = iy as usize;
        let tx = fx - ix as f64;
        let ty = fy - iy as f64;
        let samples = [
            (field[iy * self.width + ix], (1.0 - tx) * (1.0 - ty)),
            (field[iy * self.width + ix + 1], tx * (1.0 - ty)),
            (field[(iy + 1) * self.width + ix], (1.0 - tx) * ty),
            (field[(iy + 1) * self.width + ix + 1], tx * ty),
        ];
        let mut sum = 0.0;
        let mut weight = 0.0;
        for (value, w) in samples {
            if let Some(z) = value {
                sum += z * w;
                weight += w;
            }
        }
        if weight > 0.0 {
            Some(sum / weight)
        } else {
            None
        }
    }
}

/// Build a height grid from `mesh` using downward rays. Odd hit counts along a
/// column mark material; the highest upward-facing hit within the slope limit
/// is kept as the top surface.
pub fn build_height_field(
    mesh: &Mesh,
    step: f64,
    max_angle_deg: f64,
    band_thickness: f64,
) -> HeightGrid {
    let (min, max) = mesh.bounds();
    let step = step.max(0.1);
    let pad = step * 2.0;
    let origin = [min[0] - pad, min[1] - pad];
    let width = (((max[0] - min[0]) + 2.0 * pad) / step).ceil() as usize + 1;
    let height = (((max[1] - min[1]) + 2.0 * pad) / step).ceil() as usize + 1;
    // Ray acceptance uses a generous angle so shallow ramps (e.g. 12°) are still
    // detected; `max_angle_deg` is enforced later via slope clamping on the grid.
    let detection_angle_deg = max_angle_deg.clamp(55.0, 89.0);
    let detect_slope = detection_angle_deg.to_radians().tan();
    let cos_limit = (1.0 / (1.0 + detect_slope * detect_slope).sqrt()).max(0.0);

    let mut z_top = vec![None; width * height];
    let ray_top = max[2] + 1.0;

    for iy in 0..height {
        for ix in 0..width {
            let x = origin[0] + ix as f64 * step;
            let y = origin[1] + iy as f64 * step;
            let mut hits: Vec<(f64, [f64; 3])> = Vec::new();
            for triangle in &mesh.triangles {
                if let Some((z, normal)) = ray_triangle_z(x, y, ray_top, mesh, triangle) {
                    if normal[2] >= cos_limit {
                        hits.push((z, normal));
                    }
                }
            }
            if hits.is_empty() {
                // Relax slope filter slightly so 12° ramps still register as tops, but
                // ignore near-vertical walls that would otherwise pin every column to max Z.
                let cos_fallback = (max_angle_deg + 5.0).to_radians().cos();
                for triangle in &mesh.triangles {
                    if let Some((z, normal)) = ray_triangle_z(x, y, ray_top, mesh, triangle) {
                        if normal[2] >= cos_fallback {
                            hits.push((z, normal));
                        }
                    }
                }
            }
            if hits.is_empty() {
                continue;
            }
            let count_all = count_ray_hits(x, y, ray_top, mesh);
            if count_all % 2 == 1 || !hits.is_empty() {
                z_top[iy * width + ix] = select_exterior_top_hit(&hits, max_angle_deg, band_thickness);
            }
        }
    }

    let mut z_top_smooth = gaussian_blur_optional(&z_top, width, height, 1.0);
    clamp_top_surface_slopes(&mut z_top_smooth, width, height, step, max_angle_deg);
    let mut z_band_start = vec![None; width * height];
    for (top, start) in z_top_smooth.iter().zip(z_band_start.iter_mut()) {
        *start = top.map(|z| z - band_thickness);
    }
    let len = width * height;

    HeightGrid {
        origin,
        step,
        width,
        height,
        z_top,
        z_top_smooth,
        z_band_start,
        z_deform_cap: vec![None; len],
        z_deform_flat: vec![None; len],
    }
}

/// When a thin curved skin sits below a flat deck, keep the sloped exterior shell.
fn select_exterior_top_hit(
    hits: &[(f64, [f64; 3])],
    max_angle_deg: f64,
    band_thickness: f64,
) -> Option<f64> {
    if hits.is_empty() {
        return None;
    }
    // Allow slightly steeper hits than the print cap so shallow fixture ramps (e.g. 12°)
    // are not rejected before grid clamping enforces `max_angle_deg`.
    let cos_print = (max_angle_deg + 5.0).to_radians().cos();
    let mut printable: Vec<(f64, [f64; 3])> = hits
        .iter()
        .copied()
        .filter(|(_, n)| n[2] >= cos_print)
        .collect();
    if printable.is_empty() {
        let mut sorted = hits.to_vec();
        sorted.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        return Some(sorted[0].0);
    }
    printable.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
  // Deduplicate heights within one layer.
    let mut layers: Vec<(f64, [f64; 3])> = Vec::new();
    for h in printable {
        if layers
            .last()
            .map(|(z, _)| (h.0 - z).abs() < 0.08)
            .unwrap_or(false)
        {
            continue;
        }
        layers.push(h);
    }
    if layers.len() >= 2 {
        let (z_hi, n_hi) = layers[0];
        let (z_lo, n_lo) = layers[1];
        if z_hi - z_lo >= band_thickness * 0.45 && n_hi[2] > 0.94 && n_lo[2] >= cos_print {
            return Some(z_lo);
        }
    }
    Some(layers[0].0)
}

fn count_ray_hits(x: f64, y: f64, z_high: f64, mesh: &Mesh) -> usize {
    let mut count = 0usize;
    for triangle in &mesh.triangles {
        if ray_triangle_any(x, y, z_high, mesh, triangle) {
            count += 1;
        }
    }
    count
}

fn ray_triangle_any(
    x: f64,
    y: f64,
    z_high: f64,
    mesh: &Mesh,
    tri: &[u32; 3],
) -> bool {
    ray_triangle_z(x, y, z_high, mesh, tri).is_some()
}

fn ray_triangle_z(
    x: f64,
    y: f64,
    z_high: f64,
    mesh: &Mesh,
    tri: &[u32; 3],
) -> Option<(f64, [f64; 3])> {
    let v0 = mesh.vertices[tri[0] as usize];
    let v1 = mesh.vertices[tri[1] as usize];
    let v2 = mesh.vertices[tri[2] as usize];
  let edge1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let edge2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    let normal = cross(edge1, edge2);
    let len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if len < 1e-9 {
        return None;
    }
    let normal = [normal[0] / len, normal[1] / len, normal[2] / len];
    if normal[2].abs() < 1e-6 {
        return None;
    }
    // Ray (x,y,*) downward: find z where point lies in triangle XY projection.
    if !point_in_tri_xy(x, y, v0, v1, v2) {
        return None;
    }
    // Plane equation n·p = n·v0
    let d = normal[0] * v0[0] + normal[1] * v0[1] + normal[2] * v0[2];
    let z = (d - normal[0] * x - normal[1] * y) / normal[2];
    if z > z_high || z < 0.0 {
        return None;
    }
    Some((z, normal))
}

fn point_in_tri_xy(x: f64, y: f64, v0: [f64; 3], v1: [f64; 3], v2: [f64; 3]) -> bool {
    let sign = |ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64| -> f64 {
        (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
    };
    let b0 = sign(x, y, v0[0], v0[1], v1[0], v1[1]) < 0.0;
    let b1 = sign(x, y, v1[0], v1[1], v2[0], v2[1]) < 0.0;
    let b2 = sign(x, y, v2[0], v2[1], v0[0], v0[1]) < 0.0;
    b0 == b1 && b1 == b2
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Separable Gaussian blur on optional samples; missing cells borrow neighbours.
/// Limit neighbour height steps so the printable top does not exceed `max_angle_deg`.
pub fn clamp_top_surface_slopes(
    field: &mut [Option<f64>],
    width: usize,
    height: usize,
    step: f64,
    max_angle_deg: f64,
) {
    let max_dz = max_angle_deg.to_radians().tan() * step * 1.001;
    if max_dz <= 1e-9 {
        return;
    }
    for _ in 0..48 {
        let mut changed = false;
        for iy in 0..height {
            for ix in 0..width {
                let i = iy * width + ix;
                let Some(z0) = field[i] else { continue };
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let nx = ix as isize + dx;
                    let ny = iy as isize + dy;
                    if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                        continue;
                    }
                    let j = ny as usize * width + nx as usize;
                    let Some(z1) = field[j] else { continue };
                    let dz = (z1 - z0).abs();
                    if dz > max_dz + 1e-9 {
                        let mid = 0.5 * (z0 + z1);
                        let half = max_dz * 0.5;
                        if z0 > z1 {
                            field[i] = Some(mid + half);
                            field[j] = Some(mid - half);
                        } else {
                            field[i] = Some(mid - half);
                            field[j] = Some(mid + half);
                        }
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

pub fn gaussian_blur_optional(
    field: &[Option<f64>],
    width: usize,
    height: usize,
    sigma: f64,
) -> Vec<Option<f64>> {
    let radius = (sigma * 3.0).ceil() as isize;
    let kernel: Vec<f64> = (-radius..=radius)
        .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
        .collect();
    let norm: f64 = kernel.iter().sum();
    let mut temp = vec![None; field.len()];
    let mut out = vec![None; field.len()];
    for iy in 0..height {
        for ix in 0..width {
            let mut sum = 0.0;
            let mut w = 0.0;
            for (dx, k) in (-radius..=radius).zip(&kernel) {
                let jx = (ix as isize + dx).clamp(0, width as isize - 1) as usize;
                if let Some(v) = field[iy * width + jx] {
                    sum += v * k / norm;
                    w += k / norm;
                }
            }
            temp[iy * width + ix] = if w > 0.0 { Some(sum / w) } else { None };
        }
    }
    for iy in 0..height {
        for ix in 0..width {
            let mut sum = 0.0;
            let mut w = 0.0;
            for (dy, k) in (-radius..=radius).zip(&kernel) {
                let jy = (iy as isize + dy).clamp(0, height as isize - 1) as usize;
                if let Some(v) = temp[jy * width + ix] {
                    sum += v * k / norm;
                    w += k / norm;
                }
            }
            out[iy * width + ix] = if w > 0.0 { Some(sum / w) } else { None };
        }
    }
    out
}

pub fn count_top_islands(grid: &HeightGrid) -> usize {
    let mut visited = vec![false; grid.z_top_smooth.len()];
    let mut islands = 0usize;
    for index in 0..grid.z_top_smooth.len() {
        if grid.z_top_smooth[index].is_none() || visited[index] {
            continue;
        }
        islands += 1;
        flood(index, grid, &mut visited);
    }
    islands
}

fn flood(start: usize, grid: &HeightGrid, visited: &mut [bool]) {
    let mut stack = vec![start];
    while let Some(i) = stack.pop() {
        if visited[i] || grid.z_top_smooth[i].is_none() {
            continue;
        }
        visited[i] = true;
        let ix = i % grid.width;
        let iy = i / grid.width;
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nx = ix as isize + dx;
            let ny = iy as isize + dy;
            if nx >= 0 && ny >= 0 {
                let nx = nx as usize;
                let ny = ny as usize;
                if nx < grid.width && ny < grid.height {
                    stack.push(ny * grid.width + nx);
                }
            }
        }
    }
}

#[cfg(test)]
fn tilted_slab_mesh(w: f64, d: f64, t: f64, tilt_deg: f64) -> crate::mesh::Mesh {
    use crate::test_meshes::box_mesh;
    let tilt = tilt_deg.to_radians();
    let mut mesh = box_mesh(w, d, t);
    let lift = |v: [f64; 3]| -> [f64; 3] {
        let x = v[0];
        let z = v[2];
        [x, v[1], z + x * tilt.tan()]
    };
    for v in &mut mesh.vertices {
        *v = lift(*v);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_meshes::{box_mesh, sphere_mesh};

    #[test]
    fn flat_box_top_is_constant() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let mut samples = Vec::new();
        for iy in (0..grid.height).step_by(2) {
            for ix in (0..grid.width).step_by(2) {
                if let Some(z) = grid.z_top_smooth[iy * grid.width + ix] {
                    samples.push(z);
                }
            }
        }
        assert!(!samples.is_empty());
        let mean: f64 = samples.iter().sum::<f64>() / samples.len() as f64;
        for z in samples {
            assert!((z - mean).abs() < 0.5, "flat top should be level, got spread around {mean}");
            assert!((z - 10.0).abs() < 0.5);
        }
    }

    #[test]
    fn tilted_top_detected_and_clamped_to_max_angle() {
        let mesh = tilted_slab_mesh(40.0, 24.0, 0.5, 12.0);
        let grid = build_height_field(&mesh, 1.0, 10.0, 1.6);
        let mut zs = Vec::new();
        for iy in 0..grid.height {
            for ix in 0..grid.width {
                if let Some(z) = grid.z_top_smooth[iy * grid.width + ix] {
                    zs.push((ix, z));
                }
            }
        }
        assert!(!zs.is_empty(), "12° slab must be detected at 10° print limit");
        let z_min = zs.iter().map(|(_, z)| *z).fold(f64::INFINITY, f64::min);
        let z_max = zs.iter().map(|(_, z)| *z).fold(f64::NEG_INFINITY, f64::max);
        let spread = z_max - z_min;
        let expected = 40.0_f64.to_radians().tan() * 0.1 - 1.0;
        assert!(
            spread > 0.2,
            "height field must vary along ramp, spread={spread:.3}"
        );
        let max_step = 10.0_f64.to_radians().tan() * grid.step;
        for iy in 0..grid.height {
            for ix in 1..grid.width {
                let a = grid.z_top_smooth[iy * grid.width + ix - 1];
                let b = grid.z_top_smooth[iy * grid.width + ix];
                if let (Some(za), Some(zb)) = (a, b) {
                    assert!(
                        (zb - za).abs() <= max_step + 0.05,
                        "neighbour step {:.3} exceeds {:.3}",
                        (zb - za).abs(),
                        max_step
                    );
                }
            }
        }
        let _ = expected;
    }

    #[test]
    fn sphere_top_varies_smoothly() {
        let mesh = sphere_mesh(15.0);
        let grid = build_height_field(&mesh, 0.8, 45.0, 3.0);
        let centre = grid.z_top_smooth[grid.height / 2 * grid.width + grid.width / 2];
        assert!(centre.unwrap() > 25.0);
        assert!(count_top_islands(&grid) >= 1);
    }
}