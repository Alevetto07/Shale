//! Mesh feature detection for automatic non-planar placement.

use serde::{Deserialize, Serialize};

use crate::mesh::Mesh;

/// Default overhang threshold (degrees from vertical), aligned with Cura `support_angle`.
const DEFAULT_OVERHANG_DEG: f64 = 50.0;

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
    pub note: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DetectionReport {
    pub features: Vec<DetectedFeature>,
}

/// Angle between face normal and the +Z axis (0° = horizontal top, 90° = vertical wall).
fn slope_from_horizontal(nz: f64) -> f64 {
    nz.clamp(-1.0, 1.0).acos().to_degrees()
}

fn angle_from_vertical(nz: f64) -> f64 {
    nz.abs().clamp(0.0, 1.0).asin().to_degrees()
}

fn triangle_normal(mesh: &Mesh, tri: &[u32; 3]) -> [f64; 3] {
    let v0 = mesh.vertices[tri[0] as usize];
    let v1 = mesh.vertices[tri[1] as usize];
    let v2 = mesh.vertices[tri[2] as usize];
    let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-9 {
        return [0.0, 0.0, 1.0];
    }
    [n[0] / len, n[1] / len, n[2] / len]
}

fn tri_area(mesh: &Mesh, tri: &[u32; 3]) -> f64 {
    let v0 = mesh.vertices[tri[0] as usize];
    let v1 = mesh.vertices[tri[1] as usize];
    let v2 = mesh.vertices[tri[2] as usize];
    let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    let c = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    0.5 * (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
}

struct TopPatch {
    min: [f64; 3],
    max: [f64; 3],
    area: f64,
    max_slope: f64,
    height_spread: f64,
    mean_up_angle: f64,
}

/// Detect printable-top islands and underside spans on `mesh`.
pub fn detect(
    mesh: &Mesh,
    grid_step: f64,
    max_angle_deg: f64,
    band_thickness: f64,
) -> DetectionReport {
    let mut features = Vec::new();
    let mut next_id = 1u32;

    for patch in top_patches(mesh, grid_step, max_angle_deg, band_thickness) {
        let kind = classify_top(&patch, max_angle_deg);
        features.push(DetectedFeature {
            id: next_id,
            kind,
            min: patch.min,
            max: patch.max,
            area_mm2: patch.area,
            z_center_mm: 0.5 * (patch.min[2] + patch.max[2]),
            max_slope_deg: patch.max_slope,
            height_spread_mm: patch.height_spread,
            angle_from_vertical_deg: patch.mean_up_angle,
            anchor_ends: 0,
            note: top_note(kind, &patch),
        });
        next_id += 1;
    }

    for span in underside_spans(mesh, grid_step) {
        features.push(DetectedFeature {
            id: next_id,
            kind: span.kind,
            min: span.min,
            max: span.max,
            area_mm2: span.area,
            z_center_mm: span.z,
            max_slope_deg: 0.0,
            height_spread_mm: 0.0,
            angle_from_vertical_deg: span.angle,
            anchor_ends: span.anchor_ends,
            note: span.note,
        });
        next_id += 1;
    }

    DetectionReport { features }
}

fn top_note(kind: DetectedFeatureKind, patch: &TopPatch) -> String {
    match kind {
        DetectedFeatureKind::FlatTop => format!(
            "top height spread {:.2} mm, slope {:.1}° — planar shells are sufficient",
            patch.height_spread, patch.max_slope
        ),
        DetectedFeatureKind::SlopedTop => format!(
            "sloped top up to {:.1}° (spread {:.2} mm)",
            patch.max_slope, patch.height_spread
        ),
        DetectedFeatureKind::CurvedTop => format!(
            "curved top spread {:.2} mm, local slope up to {:.1}°",
            patch.height_spread, patch.max_slope
        ),
        _ => String::new(),
    }
}

fn classify_top(patch: &TopPatch, max_angle_deg: f64) -> DetectedFeatureKind {
    if patch.height_spread < 0.25 && patch.max_slope < 2.5 {
        return DetectedFeatureKind::FlatTop;
    }
    if patch.max_slope > max_angle_deg + 0.5 {
        // Steeper than machine cap — planner should skip or clamp.
        DetectedFeatureKind::SlopedTop
    } else if patch.height_spread >= 0.6 && patch.max_slope >= 3.0 {
        DetectedFeatureKind::CurvedTop
    } else if patch.max_slope >= 2.0 || patch.height_spread >= 0.35 {
        DetectedFeatureKind::SlopedTop
    } else {
        DetectedFeatureKind::FlatTop
    }
}

fn top_patches(
    mesh: &Mesh,
    step: f64,
    max_angle_deg: f64,
    band: f64,
) -> Vec<TopPatch> {
    let step = step.max(0.35);
    let (min, max) = mesh.bounds();
    let origin_x = min[0] - step;
    let origin_y = min[1] - step;
    #[derive(Default)]
    struct Cell {
        z_max: f64,
        z_min: f64,
        slope: f64,
        area: f64,
        angles: f64,
        count: u32,
    }
    let mut grid: std::collections::HashMap<(i32, i32), Cell> = std::collections::HashMap::new();

    let cos_up = (max_angle_deg + 8.0).to_radians().cos();
    for tri in &mesh.triangles {
        let n = triangle_normal(mesh, tri);
        if n[2] < cos_up.max(0.15) {
            continue;
        }
        let slope = slope_from_horizontal(n[2]);
        let area = tri_area(mesh, tri);
        for vid in tri {
            let v = mesh.vertices[*vid as usize];
            let ix = ((v[0] - origin_x) / step).floor() as i32;
            let iy = ((v[1] - origin_y) / step).floor() as i32;
            let cell = grid.entry((ix, iy)).or_default();
            if cell.count == 0 {
                cell.z_max = v[2];
                cell.z_min = v[2];
            } else {
                cell.z_max = cell.z_max.max(v[2]);
                cell.z_min = cell.z_min.min(v[2]);
            }
            cell.slope = cell.slope.max(slope);
            cell.area += area / 3.0;
            cell.angles += angle_from_vertical(n[2]);
            cell.count += 1;
        }
    }

    let mut visited = std::collections::HashSet::new();
    let mut patches = Vec::new();
    for key in grid.keys().copied() {
        if visited.contains(&key) {
            continue;
        }
        let mut stack = vec![key];
        let mut patch_min = [f64::INFINITY; 3];
        let mut patch_max = [f64::NEG_INFINITY; 3];
        let mut area = 0.0;
        let mut max_slope = 0.0f64;
        let mut z_hi = f64::NEG_INFINITY;
        let mut z_lo = f64::INFINITY;
        let mut angle_sum = 0.0;
        let mut angle_n = 0u32;

        while let Some((ix, iy)) = stack.pop() {
            if visited.contains(&(ix, iy)) {
                continue;
            }
            let Some(cell) = grid.get(&(ix, iy)) else {
                continue;
            };
            if cell.count == 0 {
                continue;
            }
            visited.insert((ix, iy));
            area += cell.area;
            max_slope = max_slope.max(cell.slope);
            z_hi = z_hi.max(cell.z_max);
            z_lo = z_lo.min(cell.z_min);
            angle_sum += cell.angles;
            angle_n += cell.count;
            let x0 = origin_x + ix as f64 * step;
            let y0 = origin_y + iy as f64 * step;
            patch_min[0] = patch_min[0].min(x0);
            patch_min[1] = patch_min[1].min(y0);
            patch_min[2] = patch_min[2].min(cell.z_min);
            patch_max[0] = patch_max[0].max(x0 + step);
            patch_max[1] = patch_max[1].max(y0 + step);
            patch_max[2] = patch_max[2].max(cell.z_max);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nb = (ix + dx, iy + dy);
                if !visited.contains(&nb) && grid.contains_key(&nb) {
                    let nb_z = grid.get(&nb).unwrap().z_max;
                    if (nb_z - cell.z_max).abs() <= band.max(step * 2.0).max(1.5) {
                        stack.push(nb);
                    }
                }
            }
        }
        if area < step * step * 0.5 {
            continue;
        }
        let height_spread = (z_hi - z_lo).max(0.0);
        let mean_up = if angle_n > 0 {
            angle_sum / angle_n as f64
        } else {
            90.0
        };
        patches.push(TopPatch {
            min: patch_min,
            max: patch_max,
            area,
            max_slope,
            height_spread,
            mean_up_angle: mean_up,
        });
    }
    patches
}

struct UndersideSpan {
    kind: DetectedFeatureKind,
    min: [f64; 3],
    max: [f64; 3],
    area: f64,
    z: f64,
    angle: f64,
    anchor_ends: u8,
    note: String,
}

fn underside_spans(mesh: &Mesh, cell_mm: f64) -> Vec<UndersideSpan> {
    let cell = cell_mm.max(0.5);
    let (min, max) = mesh.bounds();
    let origin_x = min[0];
    let origin_y = min[1];
    let _nx = ((max[0] - origin_x) / cell).ceil().max(1.0) as i32;
    let _ny = ((max[1] - origin_y) / cell).ceil().max(1.0) as i32;

    #[derive(Clone)]
    struct UnderCell {
        z: f64,
        angle: f64,
        area: f64,
    }

    let mut unders: Vec<(i32, i32, UnderCell)> = Vec::new();
    let overhang = DEFAULT_OVERHANG_DEG;
    for tri in &mesh.triangles {
        let n = triangle_normal(mesh, tri);
        if n[2] > -0.25 {
            continue;
        }
        let angle = angle_from_vertical(n[2]);
        if angle + 0.5 < overhang {
            continue;
        }
        let area = tri_area(mesh, tri);
        let cx = (mesh.vertices[tri[0] as usize][0]
            + mesh.vertices[tri[1] as usize][0]
            + mesh.vertices[tri[2] as usize][0])
            / 3.0;
        let cy = (mesh.vertices[tri[0] as usize][1]
            + mesh.vertices[tri[1] as usize][1]
            + mesh.vertices[tri[2] as usize][1])
            / 3.0;
        let cz = (mesh.vertices[tri[0] as usize][2]
            + mesh.vertices[tri[1] as usize][2]
            + mesh.vertices[tri[2] as usize][2])
            / 3.0;
        if cz < 0.4 {
            continue;
        }
        let ix = ((cx - origin_x) / cell).floor() as i32;
        let iy = ((cy - origin_y) / cell).floor() as i32;
        unders.push((
            ix,
            iy,
            UnderCell {
                z: cz,
                angle,
                area,
            },
        ));
    }

    let mut index: std::collections::HashMap<(i32, i32), Vec<usize>> = std::collections::HashMap::new();
    for (i, (ix, iy, _)) in unders.iter().enumerate() {
        index.entry((*ix, *iy)).or_default().push(i);
    }

    let mut seen = vec![false; unders.len()];
    let mut spans = Vec::new();
    for start in 0..unders.len() {
        if seen[start] {
            continue;
        }
        let mut member = Vec::new();
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(cur) = stack.pop() {
            member.push(cur);
            let (ix, iy, ref uc) = unders[cur];
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let key = (ix + dx, iy + dy);
                if let Some(list) = index.get(&key) {
                    for &next in list {
                        if seen[next] {
                            continue;
                        }
                        if (unders[next].2.z - uc.z).abs() > cell * 1.5 {
                            continue;
                        }
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        if member.is_empty() {
            continue;
        }
        let mut min_b = [f64::INFINITY; 3];
        let mut max_b = [f64::NEG_INFINITY; 3];
        let mut area = 0.0;
        let mut z_sum = 0.0;
        let mut angle_sum = 0.0;
        let mut ix_min = i32::MAX;
        let mut ix_max = i32::MIN;
        let mut iy_fix = 0i32;
        for &i in &member {
            let (ix, iy, ref c) = unders[i];
            ix_min = ix_min.min(ix);
            ix_max = ix_max.max(ix);
            iy_fix = iy;
            area += c.area;
            z_sum += c.z;
            angle_sum += c.angle;
            let x = origin_x + (ix as f64 + 0.5) * cell;
            let y = origin_y + (iy as f64 + 0.5) * cell;
            min_b[0] = min_b[0].min(x - cell * 0.5);
            min_b[1] = min_b[1].min(y - cell * 0.5);
            min_b[2] = min_b[2].min(c.z - 0.2);
            max_b[0] = max_b[0].max(x + cell * 0.5);
            max_b[1] = max_b[1].max(y + cell * 0.5);
            max_b[2] = max_b[2].max(c.z + 0.2);
        }
        if area < cell * cell * 0.8 {
            continue;
        }
        let length = (ix_max - ix_min + 1) as f64 * cell;
        let width = cell;
        let z = z_sum / member.len() as f64;
        let angle = angle_sum / member.len() as f64;
        let anchor = anchors_along_x(mesh, origin_x, origin_y, cell, ix_min, ix_max, iy_fix, z);
        let kind = if anchor >= 2 && length <= 22.0 {
            DetectedFeatureKind::Bridge
        } else if anchor == 1 {
            DetectedFeatureKind::Cantilever
        } else if anchor == 0 {
            DetectedFeatureKind::Island
        } else {
            DetectedFeatureKind::Overhang
        };
        let note = match kind {
            DetectedFeatureKind::Bridge => format!(
                "bridge span {:.1} mm at z={:.1} (anchors={anchor})",
                length, z
            ),
            DetectedFeatureKind::Cantilever => format!(
                "cantilever tip {:.1} mm long at z={:.1}",
                length, z
            ),
            DetectedFeatureKind::Island => format!("floating underside {:.1}×{:.1} mm", length, width),
            _ => format!("overhang {:.1} mm² at {:.0}° from vertical", area, angle),
        };
        spans.push(UndersideSpan {
            kind,
            min: min_b,
            max: max_b,
            area,
            z,
            angle,
            anchor_ends: anchor,
            note,
        });
    }
    spans
}

fn anchors_along_x(
    mesh: &Mesh,
    origin_x: f64,
    origin_y: f64,
    cell: f64,
    ix0: i32,
    ix1: i32,
    iy: i32,
    z: f64,
) -> u8 {
    let mut anchors = 0u8;
    for ix in [ix0, ix1] {
        let x = origin_x + (ix as f64 + 0.5) * cell;
        let y = origin_y + (iy as f64 + 0.5) * cell;
        if column_has_solid_below(mesh, x, y, z - 0.3) {
            anchors += 1;
        }
    }
    anchors
}

fn column_has_solid_below(mesh: &Mesh, x: f64, y: f64, z_test: f64) -> bool {
    let mut hits = 0usize;
    for tri in &mesh.triangles {
        let v0 = mesh.vertices[tri[0] as usize];
        let v1 = mesh.vertices[tri[1] as usize];
        let v2 = mesh.vertices[tri[2] as usize];
        let min_x = v0[0].min(v1[0]).min(v2[0]);
        let max_x = v0[0].max(v1[0]).max(v2[0]);
        let min_y = v0[1].min(v1[1]).min(v2[1]);
        let max_y = v0[1].max(v1[1]).max(v2[1]);
        if x < min_x || x > max_x || y < min_y || y > max_y {
            continue;
        }
        if point_in_tri_xy(x, y, v0, v1, v2) {
            let z = (v0[2] + v1[2] + v2[2]) / 3.0;
            if z <= z_test {
                hits += 1;
            }
        }
    }
    hits > 0
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