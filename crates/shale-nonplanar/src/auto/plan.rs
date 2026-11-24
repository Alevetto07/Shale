//! Turn detected features into printable non-planar surface bands.

use serde::{Deserialize, Serialize};

use crate::config::NonplanarOptions;
use crate::deform::DeformField;
use crate::mesh::Mesh;

use super::detect;
use super::detect::{DetectedFeature, DetectedFeatureKind, DetectionReport};
use super::field;

const DEFAULT_OVERHANG_TARGET: f64 = 45.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedSurfaceKind {
    CurvedTopBand,
    SlopedTopBand,
    BridgeTiltBand,
    CantileverTiltBand,
    OverhangTiltBand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedSurface {
    pub id: u32,
    pub feature_id: u32,
    pub kind: PlannedSurfaceKind,
    pub enabled: bool,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub tilt_deg: f64,
    pub bearing_deg: f64,
    pub extent_mm: [f64; 2],
    pub overhang_angle_before_deg: Option<f64>,
    /// Planner estimate (before minus applied tilt); not remeasured from G-code.
    pub overhang_angle_after_deg: Option<f64>,
    /// Filled after slice: angle from vertical (same unit as before/after_planned).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overhang_angle_measured_after_deg: Option<f64>,
    /// Max consecutive extrusion slope in the band (degrees from horizontal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overhang_slope_measured_from_horizontal_deg: Option<f64>,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SurfacePreview {
    pub positions: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoPlacementReport {
    pub mode: String,
    pub max_angle_deg: f64,
    pub detection: DetectionReport,
    pub surfaces: Vec<PlannedSurface>,
    pub preview: SurfacePreview,
    pub enabled_surface_count: usize,
    pub fallback_to_planar: bool,
    pub notes: Vec<String>,
}

pub fn plan_auto_placement(
    mesh: &Mesh,
    spans: &[super::types::SupportSpanInput],
    options: &NonplanarOptions,
    nominal_layer_height: f64,
) -> AutoPlacementReport {
    let band = options.effective_band_thickness(nominal_layer_height);
    let detection = detect::detect(
        mesh,
        spans,
        options.grid_step_mm,
        options.max_angle_deg,
        band,
    );
    let (surfaces, notes) = plan_surfaces(&detection, options);
    let enabled_surface_count = surfaces.iter().filter(|s| s.enabled).count();
    let fallback_to_planar = enabled_surface_count == 0;
    let preview = preview_mesh(&surfaces);
    AutoPlacementReport {
        mode: "auto".into(),
        max_angle_deg: options.max_angle_deg,
        detection,
        surfaces,
        preview,
        enabled_surface_count,
        fallback_to_planar,
        notes,
    }
}

fn plan_surfaces(
    detection: &DetectionReport,
    options: &NonplanarOptions,
) -> (Vec<PlannedSurface>, Vec<String>) {
    let max_angle_deg = options.max_angle_deg;
    let mut surfaces = Vec::new();
    let mut notes = Vec::new();
    let mut next_surface = 1u32;

    for feature in &detection.features {
        match feature.kind {
            DetectedFeatureKind::CurvedTop | DetectedFeatureKind::SlopedTop => {
                push_top_band(&mut surfaces, &mut next_surface, feature, max_angle_deg);
            }
            DetectedFeatureKind::FlatTop => {
                notes.push(format!("feature {} flat top — skipped", feature.id));
            }
            DetectedFeatureKind::Bridge => {
                plan_underside(
                    &mut surfaces,
                    &mut next_surface,
                    feature,
                    PlannedSurfaceKind::BridgeTiltBand,
                    options,
                );
            }
            DetectedFeatureKind::Cantilever => {
                plan_underside(
                    &mut surfaces,
                    &mut next_surface,
                    feature,
                    PlannedSurfaceKind::CantileverTiltBand,
                    options,
                );
            }
            DetectedFeatureKind::Overhang | DetectedFeatureKind::Island => {
                plan_underside(
                    &mut surfaces,
                    &mut next_surface,
                    feature,
                    PlannedSurfaceKind::OverhangTiltBand,
                    options,
                );
            }
            DetectedFeatureKind::Cosmetic => {
                if feature.angle_from_vertical_deg >= 52.0 {
                    plan_underside(
                        &mut surfaces,
                        &mut next_surface,
                        feature,
                        PlannedSurfaceKind::OverhangTiltBand,
                        options,
                    );
                } else {
                    notes.push(format!(
                        "feature {} cosmetic underside — planar sufficient",
                        feature.id
                    ));
                }
            }
        }
    }
    (surfaces, notes)
}

fn push_top_band(
    surfaces: &mut Vec<PlannedSurface>,
    next_surface: &mut u32,
    feature: &DetectedFeature,
    max_angle_deg: f64,
) {
    let clamp_margin = 6.0;
    let kind = if feature.kind == DetectedFeatureKind::CurvedTop {
        PlannedSurfaceKind::CurvedTopBand
    } else {
        PlannedSurfaceKind::SlopedTopBand
    };
    let unsafe_slope =
        feature.max_slope_deg > max_angle_deg + clamp_margin && feature.height_spread_mm < 2.0;
    let tilt = feature.max_slope_deg.min(max_angle_deg);
    let (enabled, reason) = if unsafe_slope {
        (
            false,
            format!(
                "max slope {:.1}° exceeds cap {:.1}° — planar fallback",
                feature.max_slope_deg, max_angle_deg
            ),
        )
    } else {
        (
            true,
            format!(
                "{}: curved-top band tilt {:.1}° (spread {:.2} mm)",
                feature.kind.name(),
                tilt,
                feature.height_spread_mm
            ),
        )
    };
    surfaces.push(PlannedSurface {
        id: *next_surface,
        feature_id: feature.id,
        kind,
        enabled,
        min: feature.min,
        max: feature.max,
        tilt_deg: tilt,
        bearing_deg: bearing_xy(feature),
        extent_mm: [
            feature.max[0] - feature.min[0],
            feature.max[1] - feature.min[1],
        ],
        overhang_angle_before_deg: feature.overhang_angle_before_deg,
        overhang_angle_after_deg: feature.overhang_angle_after_deg,
        overhang_angle_measured_after_deg: None,
        overhang_slope_measured_from_horizontal_deg: None,
        reason,
    });
    *next_surface += 1;
}

fn plan_underside(
    surfaces: &mut Vec<PlannedSurface>,
    next_surface: &mut u32,
    feature: &DetectedFeature,
    kind: PlannedSurfaceKind,
    options: &NonplanarOptions,
) {
    let max_angle_deg = options.max_angle_deg;
    let before = feature
        .overhang_angle_before_deg
        .or(Some(feature.angle_from_vertical_deg))
        .unwrap_or(60.0);
    // An underside band prints under the part, so the thing it has to clear is the
    // part itself, at the same XY: the head sweep sees the deck above the band as
    // printed material and the below-band equivalence audit has nothing planar to
    // compare against. Auto no longer plans them unless asked (M1-B3: the dome fell
    // back at its first underside surface after the band's own bookkeeping failed).
    if !options.underside_bands {
        surfaces.push(PlannedSurface {
            id: *next_surface,
            feature_id: feature.id,
            kind,
            enabled: false,
            min: feature.min,
            max: feature.max,
            tilt_deg: 0.0,
            bearing_deg: span_bearing(feature),
            extent_mm: [
                feature.max[0] - feature.min[0],
                feature.max[1] - feature.min[1],
            ],
            overhang_angle_before_deg: Some(before),
            overhang_angle_after_deg: Some(before),
            overhang_angle_measured_after_deg: None,
            overhang_slope_measured_from_horizontal_deg: None,
            reason: format!(
                "underside band off by default (asked for {before:.1}°): the part above it is \
                 material at the same XY, so the sweep and the planar audit cannot verify it — \
                 set NonplanarOptions::underside_bands to try it"
            ),
        });
        *next_surface += 1;
        return;
    }
    if before < 52.0 {
        surfaces.push(PlannedSurface {
            id: *next_surface,
            feature_id: feature.id,
            kind,
            enabled: false,
            min: feature.min,
            max: feature.max,
            tilt_deg: 0.0,
            bearing_deg: span_bearing(feature),
            extent_mm: [
                feature.max[0] - feature.min[0],
                feature.max[1] - feature.min[1],
            ],
            overhang_angle_before_deg: Some(before),
            overhang_angle_after_deg: Some(before),
            overhang_angle_measured_after_deg: None,
            overhang_slope_measured_from_horizontal_deg: None,
            reason: format!("shallow underside {before:.1}° — planar fallback"),
        });
        *next_surface += 1;
        return;
    }
    // A horizontal underside gains nothing from a tilt this pipeline can
    // deliver. The band lifts the underside in deformed space and the inverse
    // map restores it in physical Z, so the printed solid and its toolpath stay
    // planar; measured on examples/bridge-overhang.stl, `--nonplanar auto` with
    // both overhang bands enabled produced the same 3.645059 m of filament and
    // the same 1680 s estimate as `--nonplanar off` (0 flattened islands), while
    // the band's own z_band bookkeeping failed the below-band equivalence audit.
    // Claiming a tilt here is a promise the mode cannot keep, so report the
    // limit instead: a flat bridge/cantilever is spanned by the slicer's bridge
    // path, not by non-planar nozzle motion.
    if before >= 89.0 {
        surfaces.push(PlannedSurface {
            id: *next_surface,
            feature_id: feature.id,
            kind,
            enabled: false,
            min: feature.min,
            max: feature.max,
            tilt_deg: 0.0,
            bearing_deg: span_bearing(feature),
            extent_mm: [
                feature.max[0] - feature.min[0],
                feature.max[1] - feature.min[1],
            ],
            overhang_angle_before_deg: Some(before),
            overhang_angle_after_deg: Some(before),
            overhang_angle_measured_after_deg: None,
            overhang_slope_measured_from_horizontal_deg: None,
            reason: format!(
                "horizontal underside {before:.1}° — a ±{max_angle_deg:.1}° tilt round-trips to a planar toolpath; bridge printing already spans it"
            ),
        });
        *next_surface += 1;
        return;
    }
    let headroom = (before - DEFAULT_OVERHANG_TARGET).max(0.0);
    let tilt = headroom.min(max_angle_deg);
    let enabled = tilt >= 1.0 && max_angle_deg >= 1.0;
    let after = if enabled {
        (before - tilt).max(DEFAULT_OVERHANG_TARGET - 2.0)
    } else {
        before
    };
    let span_hint = if feature.overhang_angle_before_deg.is_some() {
        " — support-span target; tilt is measurable only when underside skin extrusions fall inside this band"
    } else {
        ""
    };
    let reason = if enabled {
        format!(
            "{}: tilt {:.1}° along span, overhang {before:.1}°→{after:.1}°{span_hint}",
            feature.kind.name(),
            tilt,
        )
    } else {
        format!(
            "{}: cannot reduce overhang under cap ({before:.1}°){span_hint}",
            feature.kind.name()
        )
    };
    let mut min = feature.min;
    let mut max = feature.max;
    min[2] = feature.z_center_mm - 0.5;
    max[2] = feature.z_center_mm + 2.5;
    surfaces.push(PlannedSurface {
        id: *next_surface,
        feature_id: feature.id,
        kind,
        enabled,
        min,
        max,
        tilt_deg: tilt,
        bearing_deg: span_bearing(feature),
        extent_mm: [
            feature.max[0] - feature.min[0],
            feature.max[1] - feature.min[1],
        ],
        overhang_angle_before_deg: Some(before),
        overhang_angle_after_deg: Some(after),
        overhang_angle_measured_after_deg: None,
        overhang_slope_measured_from_horizontal_deg: None,
        reason,
    });
    *next_surface += 1;
}

fn bearing_xy(feature: &DetectedFeature) -> f64 {
    let dx = feature.max[0] - feature.min[0];
    let dy = feature.max[1] - feature.min[1];
    if dx.abs() < 1e-6 {
        90.0
    } else {
        dy.atan2(dx).to_degrees().rem_euclid(360.0)
    }
}

fn span_bearing(feature: &DetectedFeature) -> f64 {
    let dx = feature.max[0] - feature.min[0];
    let dy = feature.max[1] - feature.min[1];
    if dx >= dy {
        0.0
    } else {
        90.0
    }
}

fn preview_mesh(surfaces: &[PlannedSurface]) -> SurfacePreview {
    let mut positions = Vec::new();
    for surface in surfaces.iter().filter(|s| s.enabled) {
        let z = surface.max[2];
        let corners = [
            [surface.min[0], surface.min[1], z],
            [surface.max[0], surface.min[1], z],
            [surface.max[0], surface.max[1], z],
            [surface.min[0], surface.max[1], z],
        ];
        for c in corners {
            positions.extend(c.iter().map(|v| *v as f32));
        }
    }
    SurfacePreview { positions }
}

pub fn apply_plan_to_field(field: &mut DeformField, plan: &AutoPlacementReport, layer_h: f64) {
    if plan.enabled_surface_count == 0 {
        field.island_strength.fill(0.0);
        return;
    }
    let len = field.island_strength.len();
    field.island_strength.fill(0.0);
    let origin = field.grid.origin;
    let step = field.grid.step;
    let width = field.grid.width;
    let height = field.grid.height;
    for surface in plan.surfaces.iter().filter(|s| s.enabled) {
        let written = field::apply_surface_to_grid(&mut field.grid, surface, layer_h);
        stamp_surface(origin, step, width, height, field, surface, len, &written);
    }
    let band = field.band_thickness;
    for (top, start) in field
        .grid
        .z_top_smooth
        .iter()
        .zip(field.grid.z_band_start.iter_mut())
    {
        if let Some(z) = top {
            *start = Some(start.unwrap_or(z - band));
        }
    }
}

// Grid geometry plus the field and the surface being stamped; a struct here
// would only move the same fields one level down.
#[allow(clippy::too_many_arguments)]
fn stamp_surface(
    origin: [f64; 2],
    step: f64,
    width: usize,
    height: usize,
    field: &mut DeformField,
    surface: &PlannedSurface,
    len: usize,
    written: &[bool],
) {
    for iy in 0..height {
        for ix in 0..width {
            let x = origin[0] + ix as f64 * step;
            let y = origin[1] + iy as f64 * step;
            if x < surface.min[0] - 1e-6
                || x > surface.max[0] + 1e-6
                || y < surface.min[1] - 1e-6
                || y > surface.max[1] + 1e-6
            {
                continue;
            }
            // Strength is the deformation footprint, so it follows the band the
            // surface actually wrote. Stamping the whole box gave strength to
            // columns the tilt skipped (a flank reaching above the band, or air
            // past the part) and the slope limiter then had to chamfer a multi-
            // millimetre lift step at that edge, damping the entire band.
            let i = iy * width + ix;
            if i < len && written.get(i).copied().unwrap_or(false) {
                field.island_strength[i] = 1.0;
            }
        }
    }
}
