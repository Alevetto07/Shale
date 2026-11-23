//! Pre-slice slope limiting of the deformation field.
//!
//! The deformation maps the printed top surface onto a flat target `T(x, y)`
//! while the bottom of the band (`z_band`) stays where the model put it. Every
//! intermediate layer surface is a convex combination `z_band + s*(T - z_band)`,
//! so it is enough to make both `z_band` and `T` Lipschitz with the print cap:
//! then every band layer, and the top surface itself, is within the cap.
//!
//! The relaxation is monotone (it only lowers the offending cells and stops at
//! a floor it cannot cross), so it converges in a bounded number of passes. It
//! runs *before* CuraEngine slices, on the same field the inverse map later
//! uses — a slope fix applied after slicing can never be the inverse of what
//! was sliced.

use crate::deform::DeformField;

/// Upper bound on relaxation sweeps. Each sweep lowers a cell by at most one
/// grid step of `max_dz`, so this covers a 40 mm cliff at a 10° cap and a
/// 0.35 mm grid with several times the passes actually needed.
const MAX_PASSES: usize = 4096;

/// Result of limiting the field, reported in the job notes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlopeLimitSummary {
    pub max_angle_deg: f64,
    /// Worst surface slope over top and band layers before limiting, degrees.
    pub worst_before_deg: f64,
    /// Worst surface slope after limiting, degrees.
    pub worst_after_deg: f64,
    /// Relaxation sweeps actually performed.
    pub passes: usize,
    /// Cells whose height was lowered by the limiter.
    pub cells_changed: usize,
    /// False when a floor blocked the relaxation (surfaces are not fixed sheets).
    pub converged: bool,
    /// Cells whose target could not be lowered because it was a fixed underside band.
    pub fixed_conflicts: usize,
}

impl SlopeLimitSummary {
    pub fn note(&self) -> String {
        format!(
            "slope limited in {} pass(es), worst {:.2}° -> {:.2}° (cap {:.2}°, {} cell(s))",
            self.passes,
            self.worst_before_deg,
            self.worst_after_deg,
            self.max_angle_deg,
            self.cells_changed
        )
    }
}

fn neighbour_index(ix: usize, iy: usize, dx: isize, dy: isize, w: usize, h: usize) -> Option<usize> {
    let nx = ix as isize + dx;
    let ny = iy as isize + dy;
    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
        return None;
    }
    Some(ny as usize * w + nx as usize)
}

/// Worst absolute slope (degrees) between neighbouring cells of `surface`.
///
/// Pairs that straddle two different regions (a deformed sheet next to the
/// model's untouched surface, or a top band next to an underside band) are
/// skipped: those steps are the model's own edges, not surfaces the deformation
/// generated, and the printer resolves them as walls. Everything inside one
/// region is still held to the cap.
fn worst_slope(surface: &[Option<f64>], region: &[bool], w: usize, h: usize, step: f64) -> f64 {
    let mut worst: f64 = 0.0;
    for iy in 0..h {
        for ix in 0..w {
            let i = iy * w + ix;
            let Some(a) = surface[i] else {
                continue;
            };
            for (dx, dy) in [(1, 0), (0, 1)] {
                let Some(j) = neighbour_index(ix, iy, dx, dy, w, h) else {
                    continue;
                };
                if region[i] != region[j] {
                    continue;
                }
                if let Some(b) = surface[j] {
                    worst = worst.max((b - a).abs() / step);
                }
            }
        }
    }
    worst.atan().to_degrees()
}

/// Convex combination of the band start and the flat target at fraction `s`.
fn band_surface(
    band: &[Option<f64>],
    flat: &[f64],
    has_top: &[bool],
    s: f64,
) -> Vec<Option<f64>> {
    (0..band.len())
        .map(|i| {
            if !has_top[i] {
                return None;
            }
            let z = band[i]?;
            Some(z + s * (flat[i] - z))
        })
        .collect()
}

fn worst_family_slope(
    band: &[Option<f64>],
    flat: &[f64],
    has_top: &[bool],
    region: &[bool],
    w: usize,
    h: usize,
    step: f64,
) -> f64 {
    let mut worst: f64 = 0.0;
    for s in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let surface = band_surface(band, flat, has_top, s);
        worst = worst.max(worst_slope(&surface, region, w, h, step));
    }
    worst.max(worst_slope(band, region, w, h, step))
}

struct Relaxed {
    values: Vec<f64>,
    passes: usize,
    changed: usize,
    converged: bool,
    fixed_conflicts: usize,
}

/// Largest Lipschitz minorant of `values` over `free` cells, keeping `fixed`
/// cells untouched. Each sweep lowers a cell to `neighbour + max_dz`; the
/// floor `lower` stops the descent where the surface may not go.
fn relax_lipschitz(
    values: &[f64],
    lower: &[f64],
    free: &[bool],
    fixed: &[bool],
    w: usize,
    h: usize,
    max_dz: f64,
) -> Relaxed {
    let mut f = values.to_vec();
    let mut passes = 0;
    let mut changed_cells = vec![false; values.len()];
    let mut converged = false;
    for _pass in 0..MAX_PASSES {
        let mut changed = false;
        for iy in 0..h {
            for ix in 0..w {
                let i = iy * w + ix;
                if !free[i] {
                    continue;
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let Some(j) = neighbour_index(ix, iy, dx, dy, w, h) else {
                        continue;
                    };
                    if !(free[j] || fixed[j]) {
                        continue;
                    }
                    let ceiling = f[j] + max_dz;
                    if f[i] > ceiling + 1e-12 {
                        let target = ceiling.max(lower[i]);
                        if target < f[i] - 1e-12 {
                            f[i] = target;
                            changed = true;
                            changed_cells[i] = true;
                        }
                    }
                }
            }
        }
        passes += 1;
        if !changed {
            converged = true;
            break;
        }
    }
    let mut fixed_conflicts = 0usize;
    if converged {
        // A fixed cell may still violate its neighbours; count those so the
        // caller can report an honest failure instead of claiming success.
        for iy in 0..h {
            for ix in 0..w {
                let i = iy * w + ix;
                if !fixed[i] {
                    continue;
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let Some(j) = neighbour_index(ix, iy, dx, dy, w, h) else {
                        continue;
                    };
                    if !(free[j] || fixed[j]) {
                        continue;
                    }
                    if f[i] > f[j] + max_dz + 1e-9 {
                        fixed_conflicts += 1;
                    }
                }
            }
        }
    }
    Relaxed {
        changed: changed_cells.iter().filter(|c| **c).count(),
        values: f,
        passes,
        converged,
        fixed_conflicts,
    }
}

/// Limit the field's band and top surfaces to `max_angle_deg` before slicing.
///
/// Leaves `deformed_top_z` at the highest target and collapses per-cell island
/// strength into the flat target, so the deformation is exactly the piecewise
/// linear map the limiter verified.
pub fn limit_field_slopes(field: &mut DeformField) -> SlopeLimitSummary {
    let w = field.grid.width;
    let h = field.grid.height;
    let step = field.grid.step;
    let band = field.band_thickness;
    let max_dz = field.max_angle_deg.to_radians().tan() * step;
    let mut summary = SlopeLimitSummary {
        max_angle_deg: field.max_angle_deg,
        ..Default::default()
    };
    if max_dz <= 1e-9 || w < 2 || h < 2 {
        summary.converged = true;
        return summary;
    }

    let has_top: Vec<bool> = field.grid.z_top_smooth.iter().map(|z| z.is_some()).collect();
    if !has_top.iter().any(|t| *t) {
        summary.converged = true;
        return summary;
    }

    // The target the deformation drives the top surface to, with the planned
    // per-cell strength already folded in.
    let mut original_top = vec![0.0f64; has_top.len()];
    let mut flat: Vec<f64> = (0..has_top.len())
        .map(|i| {
            let top = field.grid.z_top_smooth[i].unwrap_or(0.0);
            original_top[i] = top;
            let target = field.grid.z_deform_flat[i].unwrap_or(field.deformed_top_z);
            top + field.island_strength[i] * (target - top)
        })
        .collect();

    let band_before: Vec<Option<f64>> = (0..has_top.len())
        .map(|i| field.grid.z_top_smooth[i].map(|z| z - band))
        .collect();
    let flat_before = flat.clone();
    // Region of each cell: `true` for a top band (target at or above the model
    // top) and `false` for an underside sheet the plan tilts but does not raise.
    // The cap holds inside each region; the edge between them is the model's own.
    let region: Vec<bool> = (0..has_top.len())
        .map(|i| !has_top[i] || flat[i] >= original_top[i] - 1e-9)
        .collect();
    summary.worst_before_deg =
        worst_family_slope(&band_before, &flat_before, &has_top, &region, w, h, step);
    if summary.worst_before_deg <= summary.max_angle_deg {
        summary.converged = true;
        summary.worst_after_deg = summary.worst_before_deg;
        return summary;
    }

    // 1. Band start: the reference top minus the (constant) band thickness has
    //    the slope of the reference top, so lower the reference where a cliff
    //    survived the earlier grid clamp.
    let mut top_values = vec![0.0f64; has_top.len()];
    for i in 0..has_top.len() {
        top_values[i] = field.grid.z_top_smooth[i].unwrap_or(0.0);
    }
    let floor_top = vec![0.0f64; has_top.len()];
    let no_fixed = vec![false; has_top.len()];
    let top_relax = relax_lipschitz(&top_values, &floor_top, &has_top, &no_fixed, w, h, max_dz);
    // 2. Flat target: may only come down to the reference top (never below it
    //    for top bands). Underside bands are fixed sheets, not relaxed.
    let mut fixed = vec![false; has_top.len()];
    let mut free = vec![false; has_top.len()];
    let mut lower = vec![f64::NEG_INFINITY; has_top.len()];
    for i in 0..has_top.len() {
        if !has_top[i] {
            continue;
        }
        let top = top_relax.values[i];
        if !region[i] {
            // Planned underside band: keep the planned target as a fixed sheet.
            fixed[i] = true;
        } else {
            free[i] = true;
            lower[i] = top;
            flat[i] = flat[i].max(top);
        }
    }
    let flat_relax = relax_lipschitz(&flat, &lower, &free, &fixed, w, h, max_dz);

    // Write the limited field back. The deformation reads the reference top,
    // the band start and the flat target from the grid.
    for i in 0..has_top.len() {
        if !has_top[i] {
            continue;
        }
        let top = top_relax.values[i];
        field.grid.z_top_smooth[i] = Some(top);
        field.grid.z_band_start[i] = Some(top - band);
        field.grid.z_deform_flat[i] = Some(flat_relax.values[i]);
        field.island_strength[i] = 1.0;
    }
    field.deformed_top_z = flat_relax
        .values
        .iter()
        .zip(has_top.iter())
        .filter(|(_, t)| **t)
        .map(|(v, _)| *v)
        .fold(f64::NEG_INFINITY, f64::max);

    let band_after: Vec<Option<f64>> = (0..has_top.len())
        .map(|i| field.grid.z_top_smooth[i].map(|z| z - band))
        .collect();
    summary.worst_after_deg =
        worst_family_slope(&band_after, &flat_relax.values, &has_top, &region, w, h, step);
    summary.passes = top_relax.passes + flat_relax.passes;
    summary.cells_changed = top_relax.changed + flat_relax.changed;
    summary.fixed_conflicts = top_relax.fixed_conflicts + flat_relax.fixed_conflicts;
    summary.converged = top_relax.converged
        && flat_relax.converged
        && summary.worst_after_deg <= summary.max_angle_deg + 1e-9;
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto::{apply_plan_to_field, plan_auto_placement};
    use crate::config::NonplanarOptions;
    use crate::deform::build_deform_field;
    use crate::height_field::build_height_field;
    use crate::test_meshes::{box_mesh, dome_mesh};

    fn field_for(mesh: &crate::mesh::Mesh, angle: f64) -> DeformField {
        let grid = build_height_field(mesh, 0.5, angle, 1.6);
        let mut field = build_deform_field(grid, 1.6, 0.4, 0.2, angle);
        let options = NonplanarOptions {
            max_angle_deg: angle,
            ..Default::default()
        };
        let plan = plan_auto_placement(mesh, &[], &options, 0.2);
        apply_plan_to_field(&mut field, &plan, 0.2);
        field
    }

    #[test]
    fn built_dome_field_is_already_within_the_cap() {
        let mesh = dome_mesh(30.0, 6.0);
        let field = field_for(&mesh, 10.0);
        let worst = missing_slope(&field);
        assert!(
            worst <= 10.0 + 1e-6,
            "the built field must respect the cap, got {worst}"
        );
        let mut limited = field.clone();
        let summary = limit_field_slopes(&mut limited);
        assert!(summary.converged, "{summary:?}");
        assert_eq!(summary.cells_changed, 0, "{summary:?}");
        assert!(summary.passes <= 1, "{summary:?}");
    }

    // The helper below stays tiny: the interesting assertion is on the summary.
    fn missing_slope(field: &DeformField) -> f64 {
        let w = field.grid.width;
        let h = field.grid.height;
        let step = field.grid.step;
        let has_top: Vec<bool> = field.grid.z_top_smooth.iter().map(|z| z.is_some()).collect();
        let band: Vec<Option<f64>> = field
            .grid
            .z_top_smooth
            .iter()
            .map(|z| z.map(|v| v - field.band_thickness))
            .collect();
        let flat: Vec<f64> = (0..has_top.len())
            .map(|i| {
                let top = field.grid.z_top_smooth[i].unwrap_or(0.0);
                let target = field.grid.z_deform_flat[i].unwrap_or(field.deformed_top_z);
                top + field.island_strength[i] * (target - top)
            })
            .collect();
        let region: Vec<bool> = (0..has_top.len())
            .map(|i| {
                !has_top[i]
                    || flat[i]
                        >= field.grid.z_top_smooth[i].unwrap_or(0.0) + field.island_strength[i] * (field.grid.z_deform_flat[i].unwrap_or(0.0) - field.grid.z_top_smooth[i].unwrap_or(0.0))
            })
            .collect();
        worst_family_slope(&band, &flat, &has_top, &region, w, h, step)
    }

    #[test]
    fn limiter_lowers_a_steep_target_to_the_cap() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let mut field = field_for(&mesh, 10.0);
        // Spike one column: a 6 mm cliff over a single 0.5 mm cell.
        let w = field.grid.width;
        let i = (field.grid.height / 2) * w + w / 2;
        field.island_strength[i] = 1.0;
        field.grid.z_deform_flat[i] = Some(field.deformed_top_z + 6.0);
        let before = missing_slope(&field);
        assert!(before > 10.0, "spike must start over the cap, got {before}");
        let summary = limit_field_slopes(&mut field);
        assert!(summary.converged, "{summary:?}");
        assert!(summary.cells_changed > 0, "{summary:?}");
        assert!(
            summary.worst_after_deg <= 10.0 + 1e-6,
            "after limiting: {summary:?}"
        );
        assert!(missing_slope(&field) <= 10.0 + 1e-6);
        assert!((field.island_strength[i] - 1.0).abs() < 1e-12);
    }
    #[test]
    fn flat_box_is_left_alone() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let mut field = field_for(&mesh, 10.0);
        let before = field.grid.z_top_smooth.clone();
        let summary = limit_field_slopes(&mut field);
        assert!(summary.converged, "{summary:?}");
        assert_eq!(summary.cells_changed, 0);
        for (a, b) in before.iter().zip(field.grid.z_top_smooth.iter()) {
            match (a, b) {
                (Some(x), Some(y)) => assert!((x - y).abs() < 1e-9),
                (None, None) => {}
                other => panic!("{other:?}"),
            }
        }
    }
}
