//! Turn detected features into printable non-planar surface bands.

use serde::{Deserialize, Serialize};

use crate::config::NonplanarOptions;
use crate::deform::DeformField;
use crate::mesh::Mesh;

use super::detect::{DetectedFeature, DetectedFeatureKind, DetectionReport};
use super::detect;
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
    let detection = detect::detect(mesh, spans, options.grid_step_mm, options.max_angle_deg, band);
    let (surfaces, notes) = plan_surfaces(&detection, options.max_angle_deg);
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

fn plan_surfaces(detection: &DetectionReport, max_angle_deg: f64) -> (Vec<PlannedSurface>, Vec<String>) {
    let mut surfaces = Vec::new();
    let mut notes = Vec::new();
    let mut next_surface = 1u32;

    for feature in &detection.features {
        match feature.kind {
            DetectedFeatureKind::CurvedTop | DetectedFeatureKind::SlopedTop => {
                push_top_band(
                    &mut surfaces,
                    &mut next_surface,
                    feature,
                    max_angle_deg,
                );
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
                    max_angle_deg,
                );
            }
            DetectedFeatureKind::Cantilever => {
                plan_underside(
                    &mut surfaces,
                    &mut next_surface,
                    feature,
                    PlannedSurfaceKind::CantileverTiltBand,
                    max_angle_deg,
                );
            }
            DetectedFeatureKind::Overhang | DetectedFeatureKind::Island => {
                plan_underside(
                    &mut surfaces,
                    &mut next_surface,
                    feature,
                    PlannedSurfaceKind::OverhangTiltBand,
                    max_angle_deg,
                );
            }
            DetectedFeatureKind::Cosmetic => {
                if feature.angle_from_vertical_deg >= 52.0 {
                    plan_underside(
                        &mut surfaces,
                        &mut next_surface,
                        feature,
                        PlannedSurfaceKind::OverhangTiltBand,
                        max_angle_deg,
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
    let unsafe_slope = feature.max_slope_deg > max_angle_deg + clamp_margin
        && feature.height_spread_mm < 2.0;
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
    max_angle_deg: f64,
) {
    let before = feature
        .overhang_angle_before_deg
        .or(Some(feature.angle_from_vertical_deg))
        .unwrap_or(60.0);
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
            reason: format!(
                "shallow underside {before:.1}° — planar fallback"
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

pub fn apply_plan_to_field(
    field: &mut DeformField,
    plan: &AutoPlacementReport,
    layer_h: f64,
) {
    if plan.enabled_surface_count == 0 {
        for s in &mut field.island_strength {
            *s = 0.0;
        }
        return;
    }
    let len = field.island_strength.len();
    for s in &mut field.island_strength {
        *s = 0.0;
    }
    let origin = field.grid.origin;
    let step = field.grid.step;
    let width = field.grid.width;
    let height = field.grid.height;
    let has_top = field.grid.z_top_smooth.clone();
    for surface in plan.surfaces.iter().filter(|s| s.enabled) {
        stamp_surface(origin, step, width, height, &has_top, field, surface, len);
        field::apply_surface_to_grid(&mut field.grid, surface, layer_h);
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

fn stamp_surface(
    origin: [f64; 2],
    step: f64,
    width: usize,
    height: usize,
    has_top: &[Option<f64>],
    field: &mut DeformField,
    surface: &PlannedSurface,
    len: usize,
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
            let i = iy * width + ix;
            if i < len {
                field.island_strength[i] = 1.0;
            }
            let _ = has_top.get(i);
        }
    }
}