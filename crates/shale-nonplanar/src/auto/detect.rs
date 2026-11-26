//! Top-shell detection from the same height grid phase-1 uses.

use serde::{Deserialize, Serialize};

use crate::height_field::{build_height_field, HeightGrid};
use crate::mesh::Mesh;

use super::types::SupportSpanInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectedFeatureKind {
    CurvedTop,
    SlopedTop,
    FlatTop,
    Bridge,
    Cantilever,
    Overhang,
    Island,
    Cosmetic,
}

impl DetectedFeatureKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::CurvedTop => "curved_top",
            Self::SlopedTop => "sloped_top",
            Self::FlatTop => "flat_top",
            Self::Bridge => "bridge",
            Self::Cantilever => "cantilever",
            Self::Overhang => "overhang",
            Self::Island => "island",
            Self::Cosmetic => "cosmetic",
        }
    }

    pub fn from_support_kind(name: &str) -> Self {
        match name {
            "bridge" => Self::Bridge,
            "cantilever" => Self::Cantilever,
            "island" => Self::Island,
            "overhang" => Self::Overhang,
            "cosmetic" => Self::Cosmetic,
            _ => Self::Overhang,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectedFeature {
    pub id: u32,
    pub kind: DetectedFeatureKind,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub area_mm2: f64,
    pub z_center_mm: f64,
    pub max_slope_deg: f64,
    pub height_spread_mm: f64,
    pub angle_from_vertical_deg: f64,
    pub anchor_ends: u8,
    pub overhang_angle_before_deg: Option<f64>,
    pub overhang_angle_after_deg: Option<f64>,
    pub note: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DetectionReport {
    pub features: Vec<DetectedFeature>,
}

/// Build detection from the printable-top height grid plus `supports::analyse` spans.
pub fn detect(
    mesh: &Mesh,
    spans: &[SupportSpanInput],
    grid_step: f64,
    max_angle_deg: f64,
    band_thickness: f64,
) -> DetectionReport {
    let grid = build_height_field(mesh, grid_step, max_angle_deg, band_thickness);
    let mut features = top_island_features(&grid, mesh.bounds(), band_thickness, max_angle_deg);
    for (next_id, span) in (features.len() as u32 + 1..).zip(spans.iter()) {
        features.push(span_to_feature(next_id, span));
    }
    DetectionReport { features }
}

fn span_to_feature(id: u32, span: &SupportSpanInput) -> DetectedFeature {
    let kind = DetectedFeatureKind::from_support_kind(&span.kind);
    let (min, max) = span.bounds();
    DetectedFeature {
        id,
        kind,
        min,
        max,
        area_mm2: span.area_mm2,
        z_center_mm: span.z_mm,
        max_slope_deg: 0.0,
        height_spread_mm: 0.0,
        angle_from_vertical_deg: span.angle_from_vertical_deg,
        anchor_ends: span.anchor_ends,
        overhang_angle_before_deg: Some(span.angle_from_vertical_deg),
        overhang_angle_after_deg: None,
        note: format!(
            "{} span {:.1}×{:.1} mm at z={:.1}, anchors={}",
            span.kind, span.length_mm, span.width_mm, span.z_mm, span.anchor_ends
        ),
    }
}

struct IslandStats {
    min: [f64; 3],
    max: [f64; 3],
    z_min: f64,
    z_max: f64,
    max_slope: f64,
    cells: usize,
}

fn top_island_features(
    grid: &HeightGrid,
    bounds: ([f64; 3], [f64; 3]),
    band: f64,
    max_angle_deg: f64,
) -> Vec<DetectedFeature> {
    let mut visited = vec![false; grid.z_top_smooth.len()];
    let mut islands = Vec::new();
    for start in 0..grid.z_top_smooth.len() {
        if visited[start] || grid.z_top_smooth[start].is_none() {
            continue;
        }
        let mut stack = vec![start];
        let mut stats = IslandStats {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
            z_min: f64::INFINITY,
            z_max: f64::NEG_INFINITY,
            max_slope: 0.0,
            cells: 0,
        };
        while let Some(i) = stack.pop() {
            if visited[i] || grid.z_top_smooth[i].is_none() {
                continue;
            }
            visited[i] = true;
            let ix = i % grid.width;
            let iy = i / grid.width;
            let x = grid.origin[0] + ix as f64 * grid.step;
            let y = grid.origin[1] + iy as f64 * grid.step;
            let z = grid.z_top_smooth[i].unwrap();
            stats.cells += 1;
            stats.z_min = stats.z_min.min(z);
            stats.z_max = stats.z_max.max(z);
            stats.min[0] = stats.min[0].min(x);
            stats.min[1] = stats.min[1].min(y);
            stats.min[2] = stats.min[2].min(z);
            stats.max[0] = stats.max[0].max(x + grid.step);
            stats.max[1] = stats.max[1].max(y + grid.step);
            stats.max[2] = stats.max[2].max(z);
            stats.max_slope = stats
                .max_slope
                .max(local_slope_deg(grid, ix, iy, max_angle_deg));
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let nx = ix as isize + dx;
                let ny = iy as isize + dy;
                if nx >= 0 && ny >= 0 {
                    let nx = nx as usize;
                    let ny = ny as usize;
                    if nx < grid.width && ny < grid.height {
                        let j = ny * grid.width + nx;
                        if !visited[j] {
                            let zj = grid.z_top_smooth[j];
                            let connect = band.max(grid.step * 6.0);
                            if zj.is_some() && (zj.unwrap() - z).abs() <= connect {
                                stack.push(j);
                            }
                        }
                    }
                }
            }
        }
        if stats.cells > 2 {
            islands.push(stats);
        }
    }
    let (_, mesh_max) = bounds;
    let mut features = Vec::new();
    for (id, island) in islands.into_iter().enumerate() {
        let height_spread = island.z_max - island.z_min;
        let kind = classify_top_island(height_spread, island.max_slope, max_angle_deg);
        if matches!(kind, DetectedFeatureKind::FlatTop) && height_spread < 0.2 {
            let near_global_top = island.z_max >= mesh_max[2] - band * 0.75;
            if !near_global_top {
                continue;
            }
        }
        features.push(DetectedFeature {
            id: (id + 1) as u32,
            kind,
            min: island.min,
            max: island.max,
            area_mm2: island.cells as f64 * grid.step * grid.step,
            z_center_mm: 0.5 * (island.z_min + island.z_max),
            max_slope_deg: island.max_slope,
            height_spread_mm: height_spread,
            angle_from_vertical_deg: (90.0 - island.max_slope).max(0.0),
            anchor_ends: 0,
            overhang_angle_before_deg: None,
            overhang_angle_after_deg: None,
            note: top_note(kind, height_spread, island.max_slope),
        });
    }
    features
}

fn local_slope_deg(grid: &HeightGrid, ix: usize, iy: usize, _cap: f64) -> f64 {
    let z0 = match grid.z_top_smooth[iy * grid.width + ix] {
        Some(z) => z,
        None => return 0.0,
    };
    let mut max_slope = 0.0f64;
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let nx = ix as isize + dx;
        let ny = iy as isize + dy;
        if nx < 0 || ny < 0 {
            continue;
        }
        let nx = nx as usize;
        let ny = ny as usize;
        if nx >= grid.width || ny >= grid.height {
            continue;
        }
        if let Some(z1) = grid.z_top_smooth[ny * grid.width + nx] {
            let horiz = grid.step;
            let dz = (z1 - z0).abs();
            if horiz > 1e-6 {
                max_slope = max_slope.max(dz.atan2(horiz).to_degrees());
            }
        }
    }
    max_slope
}

fn top_note(kind: DetectedFeatureKind, spread: f64, slope: f64) -> String {
    match kind {
        DetectedFeatureKind::FlatTop => {
            format!("top height spread {spread:.2} mm, slope {slope:.1}°")
        }
        DetectedFeatureKind::SlopedTop => format!("sloped top up to {slope:.1}° (spread {spread:.2} mm)"),
        DetectedFeatureKind::CurvedTop => {
            format!("curved top spread {spread:.2} mm, local slope up to {slope:.1}°")
        }
        _ => String::new(),
    }
}

fn classify_top_island(spread: f64, max_slope: f64, max_angle_deg: f64) -> DetectedFeatureKind {
    if spread < 0.25 && max_slope < 2.5 {
        DetectedFeatureKind::FlatTop
    } else if max_slope > max_angle_deg + 0.5 {
        DetectedFeatureKind::SlopedTop
    } else if spread >= 0.6 && max_slope >= 3.0 {
        DetectedFeatureKind::CurvedTop
    } else if max_slope >= 2.0 || spread >= 0.35 {
        DetectedFeatureKind::SlopedTop
    } else {
        DetectedFeatureKind::FlatTop
    }
}