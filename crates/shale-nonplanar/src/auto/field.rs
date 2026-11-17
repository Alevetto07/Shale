//! Apply planned tilts to the height grid (before deform).

use crate::height_field::HeightGrid;

use super::plan::PlannedSurface;
use super::plan::PlannedSurfaceKind;

/// Tilt or inject printable height samples for a planned surface.
pub fn apply_surface_to_grid(grid: &mut HeightGrid, surface: &PlannedSurface, layer_h: f64) {
    let (ux, uy) = bearing_unit(surface.bearing_deg);
    let tan_tilt = surface.tilt_deg.to_radians().tan().clamp(-0.18, 0.18);
    let footprint_z = surface.min[2].max(layer_h);
    for iy in 0..grid.height {
        for ix in 0..grid.width {
            let x = grid.origin[0] + ix as f64 * grid.step;
            let y = grid.origin[1] + iy as f64 * grid.step;
            if !point_in_box(x, y, surface.min, surface.max) {
                continue;
            }
            let i = iy * grid.width + ix;
            let along = (x - surface.min[0]) * ux + (y - surface.min[1]) * uy;
            let dz = along * tan_tilt;
            match surface.kind {
                PlannedSurfaceKind::CurvedTopBand | PlannedSurfaceKind::SlopedTopBand => {
                    if let Some(z) = grid.z_top_smooth[i] {
                        grid.z_top_smooth[i] = Some(z + dz);
                    }
                }
                PlannedSurfaceKind::BridgeTiltBand
                | PlannedSurfaceKind::CantileverTiltBand
                | PlannedSurfaceKind::OverhangTiltBand => {
                    let base = grid.z_top_smooth[i].unwrap_or(footprint_z);
                    grid.z_top_smooth[i] = Some(base + dz);
                    if grid.z_band_start[i].is_none() {
                        grid.z_band_start[i] = Some(base - layer_h * 6.0);
                    }
                }
            }
        }
    }
    let band = footprint_z - layer_h * 6.0;
    for (top, start) in grid.z_top_smooth.iter_mut().zip(grid.z_band_start.iter_mut()) {
        if let Some(z) = *top {
            if start.is_none() {
                *start = Some(z - layer_h * 6.0);
            }
            let _ = band;
        }
    }
}

fn point_in_box(x: f64, y: f64, min: [f64; 3], max: [f64; 3]) -> bool {
    x >= min[0] - 1e-6
        && x <= max[0] + 1e-6
        && y >= min[1] - 1e-6
        && y <= max[1] + 1e-6
}

fn bearing_unit(deg: f64) -> (f64, f64) {
    let r = deg.to_radians();
    (r.cos(), r.sin())
}