//! Apply the auto plan's tilts to the height grid, before the deform.
//!
//! Two kinds of band reach this function:
//!
//! * `CurvedTopBand` / `SlopedTopBand` — deliberately a no-op here. The deform
//!   already flattens the model's own top in deformed space and the inverse map
//!   restores physical Z, and that round trip is what makes the slicer cut
//!   in-layer sloped toolpaths. Adding a slope on top of the flat target (the
//!   old `z_top_smooth += along * tan_tilt` behaviour) replaced the target with
//!   a tilting plane: the T-wing deck was shaved by one layer and 1.5 mm of
//!   geometry came out above the model top (Z 25.48 on a 24.0 mm part) while the
//!   toolpath stayed planar — exactly "auto changes nothing", plus a
//!   head-envelope violation. The plan still decides *whether* the top island is
//!   worth flattening; the field itself is left to the default deform.
//!
//! * `BridgeTiltBand` / `CantileverTiltBand` / `OverhangTiltBand` — rewrite the
//!   column so the span's bottom skin is cut at the tilted height:
//!   `z_top_smooth` follows the tilt, the band starts six layers below it,
//!   `z_deform_cap` keeps the solid above the span untouched and
//!   `z_deform_flat` is the high end of the tilt. The tilt shows up in deformed
//!   space (asserted by `auto::tests::underside_band_tilts_bridge_bottom_in_mesh`);
//!   on a horizontal span the inverse map restores the model surface, so the
//!   printed solid is unchanged — which is why `plan_underside` refuses 90°
//!   undersides instead of promising a tilt it cannot deliver.

use crate::height_field::HeightGrid;

use super::plan::PlannedSurface;
use super::plan::PlannedSurfaceKind;

/// Tilt or inject printable height samples for a planned surface.
pub fn apply_surface_to_grid(grid: &mut HeightGrid, surface: &PlannedSurface, layer_h: f64) {
    let (ux, uy) = bearing_unit(surface.bearing_deg);
    let tan_tilt = surface.tilt_deg.to_radians().tan().clamp(-0.18, 0.18);
    let top_band = matches!(
        surface.kind,
        PlannedSurfaceKind::CurvedTopBand | PlannedSurfaceKind::SlopedTopBand
    );
    let max_along = surface_max_along_bearing(surface, ux, uy);
    if !top_band {
        for iy in 0..grid.height {
            for ix in 0..grid.width {
                let x = grid.origin[0] + ix as f64 * grid.step;
                let y = grid.origin[1] + iy as f64 * grid.step;
                if !point_in_box(x, y, surface.min, surface.max) {
                    continue;
                }
                let i = iy * grid.width + ix;
                let along = (x - surface.min[0]) * ux + (y - surface.min[1]) * uy;
                let underside_z = surface.min[2] + 0.5;
                grid.z_top_smooth[i] = Some(underside_z + along * tan_tilt);
                grid.z_band_start[i] = Some((underside_z - layer_h * 6.0).max(0.0));
                grid.z_deform_cap[i] = Some(surface.max[2]);
                grid.z_deform_flat[i] = Some(underside_z + max_along * tan_tilt);
            }
        }
    }
    for (top, start) in grid
        .z_top_smooth
        .iter_mut()
        .zip(grid.z_band_start.iter_mut())
    {
        if let Some(z) = *top {
            if start.is_none() {
                *start = Some(z - layer_h * 6.0);
            }
        }
    }
}

fn point_in_box(x: f64, y: f64, min: [f64; 3], max: [f64; 3]) -> bool {
    x >= min[0] - 1e-6 && x <= max[0] + 1e-6 && y >= min[1] - 1e-6 && y <= max[1] + 1e-6
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
