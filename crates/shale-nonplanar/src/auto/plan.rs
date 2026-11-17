//! Turn detected features into printable non-planar surface bands.

use serde::{Deserialize, Serialize};

use crate::config::NonplanarOptions;
use crate::deform::DeformField;
use crate::mesh::Mesh;

use super::detect::{DetectedFeature, DetectedFeatureKind, DetectionReport};
use super::detect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedSurfaceKind {
    CurvedTopBand,
    SlopedTopBand,
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
    pub reason: String,
}

/// Axis-aligned preview quads (`x,y,z` triplets, four corners per surface).
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

/// Full automatic placement pipeline: detect → plan surfaces → strength mask.
pub fn plan_auto_placement(
    mesh: &Mesh,
    options: &NonplanarOptions,
    nominal_layer_height: f64,
) -> AutoPlacementReport {
    let band = options.effective_band_thickness(nominal_layer_height);
    let detection = detect::detect(mesh, options.grid_step_mm, options.max_angle_deg, band);
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
                let clamp_margin = 6.0;
                let unsafe_slope = feature.max_slope_deg > max_angle_deg + clamp_margin;
                let (enabled, reason) = if unsafe_slope {
                    (
                        false,
                        format!(
                            "max slope {:.1}° exceeds cap {:.1}° — falling back to planar",
                            feature.max_slope_deg, max_angle_deg
                        ),
                    )
                } else {
                    let kind = if feature.kind == DetectedFeatureKind::CurvedTop {
                        PlannedSurfaceKind::CurvedTopBand
                    } else {
                        PlannedSurfaceKind::SlopedTopBand
                    };
                    (
                        true,
                        if feature.max_slope_deg > max_angle_deg + 1e-3 {
                            format!(
                                "{}: enable {} with slope clamped {:.1}°→{:.1}°",
                                feature.kind.name(),
                                if kind == PlannedSurfaceKind::CurvedTopBand {
                                    "curved_top_band"
                                } else {
                                    "sloped_top_band"
                                },
                                feature.max_slope_deg,
                                max_angle_deg
                            )
                        } else {
                            format!(
                                "{}: enable {} (spread {:.2} mm, slope {:.1}°)",
                                feature.kind.name(),
                                if kind == PlannedSurfaceKind::CurvedTopBand {
                                    "curved_top_band"
                                } else {
                                    "sloped_top_band"
                                },
                                feature.height_spread_mm,
                                feature.max_slope_deg
                            )
                        },
                    )
                };
                let kind = if feature.kind == DetectedFeatureKind::CurvedTop {
                    PlannedSurfaceKind::CurvedTopBand
                } else {
                    PlannedSurfaceKind::SlopedTopBand
                };
                surfaces.push(PlannedSurface {
                    id: next_surface,
                    feature_id: feature.id,
                    kind,
                    enabled,
                    min: feature.min,
                    max: feature.max,
                    tilt_deg: feature.max_slope_deg.min(max_angle_deg),
                    bearing_deg: bearing_xy(feature),
                    extent_mm: [
                        feature.max[0] - feature.min[0],
                        feature.max[1] - feature.min[1],
                    ],
                    reason,
                });
                next_surface += 1;
            }
            DetectedFeatureKind::FlatTop => {
                notes.push(format!(
                    "feature {} flat top — skipped ({})",
                    feature.id, feature.note
                ));
            }
            DetectedFeatureKind::Bridge => {
                notes.push(format!(
                    "feature {} bridge detected — planar layers keep short spans ({})",
                    feature.id, feature.note
                ));
            }
            DetectedFeatureKind::Cantilever => {
                // Underside cantilever: curved-top pass does not replace support; skip band unless a sloped top sits above.
                notes.push(format!(
                    "feature {} cantilever — underside only; no curved-top band ({})",
                    feature.id, feature.note
                ));
            }
            DetectedFeatureKind::Island | DetectedFeatureKind::Overhang => {
                notes.push(format!(
                    "feature {} {} — needs support, not curved-top deformation ({})",
                    feature.id,
                    feature.kind.name(),
                    feature.note
                ));
            }
        }
    }
    (surfaces, notes)
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

/// Paint planned surfaces onto `field.island_strength` (0 = planar, 1 = full curved-top band).
pub fn apply_plan_to_field(field: &mut DeformField, plan: &AutoPlacementReport) {
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
            if i < len && has_top[i].is_some() {
                field.island_strength[i] = 1.0;
            }
        }
    }
}