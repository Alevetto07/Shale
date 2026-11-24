//! Apply planned tilts to the height grid (before deform).
//!
//! Top bands are deliberately a no-op here. The deform already flattens the
//! model's own top in deformed space and the inverse map restores physical Z,
//! and that round trip is what makes the slicer cut in-layer sloped toolpaths.
//! Adding a slope on top of the flat target (the old `z_top_smooth += dz`
//! behaviour) replaced the target with a tilting plane: on examples/t-wing.stl
//! the deck lost a layer, 1.5 mm of geometry came out above a 24.0 mm part
//! (Z 25.48, a head-envelope violation) and the toolpath stayed planar — the
//! "auto changes nothing" symptom. The plan still decides *whether* a top
//! island is worth flattening; the field itself keeps the default deform.
//!
//! Underside bands are untouched: they still write the tilted column. Whether a
//! real underside tilt can survive the inverse map is the field rework's
//! business (see M1-C-REPORT.md, "Requests to other agents").

use crate::height_field::HeightGrid;

use super::plan::PlannedSurface;
use super::plan::PlannedSurfaceKind;

/// Tilt or inject printable height samples for a planned surface.
pub fn apply_surface_to_grid(grid: &mut HeightGrid, surface: &PlannedSurface, layer_h: f64) {
    let (ux, uy) = bearing_unit(surface.bearing_deg);
    let tan_tilt = surface.tilt_deg.to_radians().tan().clamp(-0.18, 0.18);
    let footprint_z = surface.min[2].max(layer_h);
    let max_along = surface_max_along_bearing(surface, ux, uy);
    for iy in 0..grid.height {
        for ix in 0..grid.width {
            let x = grid.origin[0] + ix as f64 * grid.step;
            let y = grid.origin[1] + iy as f64 * grid.step;
            if !point_in_box(x, y, surface.min, surface.max) {
                continue;
            }
            let i = iy * grid.width + ix;
            let along = (x - surface.min[0]) * ux + (y - surface.min[1]) * uy;
            match surface.kind {
                PlannedSurfaceKind::CurvedTopBand | PlannedSurfaceKind::SlopedTopBand => {
                    // No slope injection: the deform's flatten + inverse map already
                    // turns this into an in-layer sloped toolpath (see module doc).
                }
                PlannedSurfaceKind::BridgeTiltBand
                | PlannedSurfaceKind::CantileverTiltBand
                | PlannedSurfaceKind::OverhangTiltBand => {
                    let underside_z = surface.min[2] + 0.5;
                    let tilted = underside_z + along * tan_tilt;
                    // Only tilt where the column top sits inside this band, i.e. a thin
                    // skin over air (a bridge deck). Where the column reaches far above
                    // the band — a curved flank — writing the underside height as the
                    // column's top squashes the model, and the deform then flattens the
                    // part to repair the head envelope: measured on examples/dome.stl an
                    // enabled 61.3° band rewrote 1771 column tops and turned the dome into
                    // a plateau (+81 % filament, planar-equivalence audit failed).
                    let top = grid.z_top_smooth[i].unwrap_or(footprint_z);
                    let margin = layer_h * 6.0;
                    if top > surface.max[2].max(footprint_z) + margin {
                        continue;
                    }
                    grid.z_top_smooth[i] = Some(tilted);
                    grid.z_band_start[i] = Some((underside_z - layer_h * 6.0).max(0.0));
                    grid.z_deform_cap[i] = Some(surface.max[2]);
                    grid.z_deform_flat[i] = Some(underside_z + max_along * tan_tilt);
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

fn surface_max_along_bearing(surface: &PlannedSurface, ux: f64, uy: f64) -> f64 {
  let corners = [
        [surface.min[0], surface.min[1]],
        [surface.max[0], surface.min[1]],
        [surface.max[0], surface.max[1]],
        [surface.min[0], surface.max[1]],
    ];
    corners
        .iter()
        .map(|[x, y]| (x - surface.min[0]) * ux + (y - surface.min[1]) * uy)
        .fold(0.0_f64, f64::max)
}