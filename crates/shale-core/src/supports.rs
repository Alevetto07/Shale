//! Overhang, bridge, and cantilever analysis.
//!
//! The grid is the part a later non-planar planner can reuse: each span says
//! whether an underside is held at both ends (a bridge), at one end (a
//! cantilever), or not at all (an island). Support placement is a decision on
//! top of that map. It does not modify CuraEngine. Proposed columns become
//! `support_mesh` volumes and avoided spans become `anti_overhang_mesh`
//! blockers, which the 5.13 CLI accepts as per-object settings.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::mesh::TriangleMesh;

/// Cura's `support_angle` default, in degrees from vertical.
pub const DEFAULT_SUPPORT_ANGLE: f64 = 50.0;

/// Cura's overhang angle is measured from the vertical. `0` supports every
/// underside, `90` supports none. The default of 50 matches `support_angle`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupportParams {
    #[serde(default = "default_angle")]
    pub overhang_angle_deg: f64,
    #[serde(default = "default_bridge")]
    pub max_bridge_mm: f64,
    #[serde(default = "default_cell")]
    pub cell_mm: f64,
    /// Undersides this close to the threshold, and large enough, are treated
    /// as cosmetic: support would scar them and the slope can carry itself.
    #[serde(default = "default_band")]
    pub cosmetic_band_deg: f64,
    #[serde(default = "default_cosmetic_area")]
    pub cosmetic_min_area_mm2: f64,
    #[serde(default = "default_min_area")]
    pub min_region_area_mm2: f64,
}

fn default_angle() -> f64 {
    DEFAULT_SUPPORT_ANGLE
}
fn default_bridge() -> f64 {
    20.0
}
fn default_cell() -> f64 {
    1.0
}
fn default_band() -> f64 {
    8.0
}
fn default_cosmetic_area() -> f64 {
    30.0
}
fn default_min_area() -> f64 {
    2.0
}

impl Default for SupportParams {
    fn default() -> Self {
        Self {
            overhang_angle_deg: default_angle(),
            max_bridge_mm: default_bridge(),
            cell_mm: default_cell(),
            cosmetic_band_deg: default_band(),
            cosmetic_min_area_mm2: default_cosmetic_area(),
            min_region_area_mm2: default_min_area(),
        }
    }
}

/// What the underside is, independent of whether support is wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKind {
    /// Held at both ends and short enough to span without support.
    Bridge,
    /// Held at one end. The free end is a tip.
    Cantilever,
    /// No supporting material beside it.
    Island,
    /// Needs support, and is not one of the shapes above.
    Overhang,
    /// Shallow attached underside. Support would mark a face that can carry itself.
    Cosmetic,
}

impl FeatureKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bridge => "bridge",
            Self::Cantilever => "cantilever",
            Self::Island => "island",
            Self::Overhang => "overhang",
            Self::Cosmetic => "cosmetic",
        }
    }

    pub fn wants_support(self) -> bool {
        matches!(self, Self::Cantilever | Self::Island | Self::Overhang)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionRole {
    /// `support_mesh` column. CuraEngine fills it with support.
    Enforce,
    /// `anti_overhang_mesh` slab. CuraEngine ignores overhangs inside it.
    Block,
}

impl RegionRole {
    pub fn name(self) -> &'static str {
        match self {
            Self::Enforce => "enforce",
            Self::Block => "block",
        }
    }
}

/// One sampled underside point. A non-planar planner can fit a plane to these.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SurfaceSample {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub angle_from_vertical_deg: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Span {
    pub kind: FeatureKind,
    pub length_mm: f64,
    pub width_mm: f64,
    pub area_mm2: f64,
    pub z_mm: f64,
    pub angle_from_vertical_deg: f64,
    /// 0 island, 1 cantilever, 2 bridge (both ends of the long axis).
    pub anchor_ends: u8,
    pub samples: Vec<SurfaceSample>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Analysis {
    pub spans: Vec<Span>,
}

impl Analysis {
    pub fn count(&self, kind: FeatureKind) -> usize {
        self.spans.iter().filter(|span| span.kind == kind).count()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupportRegion {
    pub id: u32,
    pub kind: FeatureKind,
    pub role: RegionRole,
    pub enabled: bool,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub contact_area_mm2: f64,
    pub volume_mm3: f64,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupportComparison {
    pub proposed_volume_mm3: f64,
    pub proposed_contact_mm2: f64,
    pub proposed_time_seconds: f64,
    pub everywhere_volume_mm3: f64,
    pub everywhere_contact_mm2: f64,
    pub everywhere_time_seconds: f64,
    /// True when the times came from two CuraEngine slices rather than the
    /// bead estimate in [`estimate_time`].
    pub times_from_engine: bool,
    #[serde(default)]
    pub proposed_support_metres: Option<f64>,
    #[serde(default)]
    pub everywhere_support_metres: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupportPlan {
    pub params: SupportParams,
    pub regions: Vec<SupportRegion>,
    pub comparison: SupportComparison,
    pub spans: Vec<Span>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupportOverlay {
    pub id: u32,
    pub role: String,
    pub positions: Vec<f32>,
}

/// Angle of a face from a vertical wall, in degrees. A horizontal underside
/// is 90. A vertical wall is 0.
pub fn angle_from_vertical(normal_z: f64) -> f64 {
    normal_z.abs().clamp(0.0, 1.0).asin().to_degrees()
}

pub fn analyse(mesh: &TriangleMesh, params: &SupportParams) -> Analysis {
    Analysis {
        spans: components(mesh, params)
            .into_iter()
            .map(|component| component.span)
            .collect(),
    }
}

pub fn propose(mesh: &TriangleMesh, params: &SupportParams) -> SupportPlan {
    let params = sanitise(params);
    let found = components(mesh, &params);
    let everywhere = everywhere_totals(&found, &params);
    let mut regions = Vec::new();
    let mut next_id = 1u32;
    // One cantilever made of two triangles arrives as two spans with the same
    // footprint. Emitting both doubles the volume the panel reports and hands
    // the engine a second box on top of the first.
    let mut placed: Vec<[f64; 6]> = Vec::new();
    for component in &found {
        if component.span.area_mm2 < params.min_region_area_mm2 {
            continue;
        }
        let role = if component.span.kind.wants_support() {
            RegionRole::Enforce
        } else {
            RegionRole::Block
        };
        for rect in &component.rects {
            let (min, max, volume) = match role {
                RegionRole::Enforce => {
                    let top = rect.z_top - 0.2;
                    let bottom = if rect.z_floor > 0.05 {
                        rect.z_floor + 0.2
                    } else {
                        0.0
                    };
                    if top - bottom < 0.4 {
                        continue;
                    }
                    let min = [rect.x0, rect.y0, bottom];
                    let max = [rect.x1, rect.y1, top];
                    let volume = (rect.x1 - rect.x0) * (rect.y1 - rect.y0) * (top - bottom);
                    (min, max, volume)
                }
                RegionRole::Block => {
                    let min = [rect.x0, rect.y0, rect.z_top - 1.2];
                    let max = [rect.x1, rect.y1, rect.z_top + 0.6];
                    (min, max, 0.0)
                }
            };
            let bounds = [min[0], min[1], min[2], max[0], max[1], max[2]];
            if placed.iter().any(|other| {
                other
                    .iter()
                    .zip(bounds.iter())
                    .all(|(a, b)| (a - b).abs() < 0.01)
            }) {
                continue;
            }
            placed.push(bounds);
            let id = next_id;
            next_id += 1;
            let contact = if role == RegionRole::Enforce {
                (rect.x1 - rect.x0) * (rect.y1 - rect.y0)
            } else {
                0.0
            };
            regions.push(SupportRegion {
                id,
                kind: component.span.kind,
                role,
                enabled: true,
                min,
                max,
                contact_area_mm2: contact,
                volume_mm3: volume,
                summary: summary_line(component.span.kind, role, &component.span, volume),
            });
        }
    }
    let comparison = comparison_of(&regions, everywhere);
    SupportPlan {
        params,
        regions,
        comparison,
        spans: found.into_iter().map(|component| component.span).collect(),
    }
}

pub fn set_enabled(plan: &mut SupportPlan, ids: Option<&[u32]>) {
    if let Some(ids) = ids {
        for region in &mut plan.regions {
            region.enabled = ids.contains(&region.id);
        }
    } else {
        for region in &mut plan.regions {
            region.enabled = true;
        }
    }
    plan.comparison.times_from_engine = false;
    plan.comparison.proposed_support_metres = None;
    plan.comparison.proposed_volume_mm3 = plan
        .regions
        .iter()
        .filter(|region| region.enabled && region.role == RegionRole::Enforce)
        .map(|region| region.volume_mm3)
        .sum();
    plan.comparison.proposed_contact_mm2 = plan
        .regions
        .iter()
        .filter(|region| region.enabled && region.role == RegionRole::Enforce)
        .map(|region| region.contact_area_mm2)
        .sum();
    plan.comparison.proposed_time_seconds = estimate_time(plan.comparison.proposed_volume_mm3);
}

pub fn overlays(plan: &SupportPlan) -> Vec<SupportOverlay> {
    plan.regions
        .iter()
        .filter(|region| region.enabled)
        .map(|region| SupportOverlay {
            id: region.id,
            role: region.role.name().to_string(),
            positions: box_positions(region.min, region.max),
        })
        .collect()
}

pub fn modifier_settings(role: RegionRole) -> Vec<(String, String)> {
    match role {
        RegionRole::Enforce => vec![
            ("support_mesh".into(), "true".into()),
            ("support_mesh_drop_down".into(), "false".into()),
        ],
        RegionRole::Block => vec![("anti_overhang_mesh".into(), "true".into())],
    }
}

pub fn region_mesh(region: &SupportRegion) -> TriangleMesh {
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    append_box(
        &mut mesh,
        [
            region.min[0] as f32,
            region.min[1] as f32,
            region.min[2] as f32,
        ],
        [
            region.max[0] as f32,
            region.max[1] as f32,
            region.max[2] as f32,
        ],
    );
    mesh
}

/// Rough print time for a support volume. 15% density, a 0.4 × 0.2 mm bead,
/// 40 mm/s. A later engine compare replaces this.
pub fn estimate_time(volume_mm3: f64) -> f64 {
    if volume_mm3 <= 0.0 {
        return 0.0;
    }
    let bead = 0.4 * 0.2;
    (volume_mm3 * 0.15) / (bead * 40.0)
}

fn sanitise(params: &SupportParams) -> SupportParams {
    let mut params = params.clone();
    params.overhang_angle_deg = params.overhang_angle_deg.clamp(0.0, 90.0);
    params.max_bridge_mm = params.max_bridge_mm.max(0.0);
    params.cell_mm = params.cell_mm.clamp(0.4, 4.0);
    params.cosmetic_band_deg = params.cosmetic_band_deg.clamp(0.0, 40.0);
    params.cosmetic_min_area_mm2 = params.cosmetic_min_area_mm2.max(0.0);
    params.min_region_area_mm2 = params.min_region_area_mm2.max(0.0);
    params
}

fn summary_line(kind: FeatureKind, role: RegionRole, span: &Span, volume: f64) -> String {
    match role {
        RegionRole::Block => format!(
            "{} {:.0} mm, {:.0} mm², blocked",
            kind.name(),
            span.length_mm,
            span.area_mm2
        ),
        RegionRole::Enforce => format!(
            "{} {:.0} mm, {:.0} mm³",
            kind.name(),
            span.length_mm,
            volume
        ),
    }
}

fn comparison_of(regions: &[SupportRegion], everywhere: (f64, f64)) -> SupportComparison {
    let proposed_volume: f64 = regions
        .iter()
        .filter(|region| region.enabled && region.role == RegionRole::Enforce)
        .map(|region| region.volume_mm3)
        .sum();
    let proposed_contact: f64 = regions
        .iter()
        .filter(|region| region.enabled && region.role == RegionRole::Enforce)
        .map(|region| region.contact_area_mm2)
        .sum();
    SupportComparison {
        proposed_volume_mm3: proposed_volume,
        proposed_contact_mm2: proposed_contact,
        proposed_time_seconds: estimate_time(proposed_volume),
        everywhere_volume_mm3: everywhere.0,
        everywhere_contact_mm2: everywhere.1,
        everywhere_time_seconds: estimate_time(everywhere.0),
        times_from_engine: false,
        proposed_support_metres: None,
        everywhere_support_metres: None,
    }
}

struct Cell {
    ix: i32,
    iy: i32,
    z: f64,
    angle: f64,
    floor: f64,
}

struct Rect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    z_top: f64,
    z_floor: f64,
}

struct Run {
    iy: i32,
    ix0: i32,
    ix1: i32,
    z: f64,
    floor: f64,
}

struct Component {
    span: Span,
    rects: Vec<Rect>,
}

fn components(mesh: &TriangleMesh, params: &SupportParams) -> Vec<Component> {
    let params = sanitise(params);
    if mesh.triangles.is_empty() {
        return Vec::new();
    }
    let (min, max) = mesh.bounds();
    let origin_x = min[0] as f64;
    let origin_y = min[1] as f64;
    let nx = ((max[0] as f64 - origin_x) / params.cell_mm)
        .ceil()
        .max(1.0) as i32;
    let ny = ((max[1] as f64 - origin_y) / params.cell_mm)
        .ceil()
        .max(1.0) as i32;
    let mut cells = Vec::new();
    let mut solids: HashMap<(i32, i32), Vec<(f64, f64)>> = HashMap::new();
    for iy in 0..ny {
        for ix in 0..nx {
            let x = origin_x + (ix as f64 + 0.5) * params.cell_mm;
            let y = origin_y + (iy as f64 + 0.5) * params.cell_mm;
            let (intervals, undersides) = column(mesh, x, y);
            if !intervals.is_empty() {
                solids.insert((ix, iy), intervals);
            }
            for (z, angle) in undersides {
                if z <= 0.75 {
                    continue;
                }
                if angle + 0.5 < params.overhang_angle_deg {
                    continue;
                }
                let floor = floor_under(&solids, ix, iy, z);
                cells.push(Cell {
                    ix,
                    iy,
                    z,
                    angle,
                    floor,
                });
            }
        }
    }

    let mut index: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (id, cell) in cells.iter().enumerate() {
        index.entry((cell.ix, cell.iy)).or_default().push(id);
    }
    let mut seen = vec![false; cells.len()];
    let mut out = Vec::new();
    for start in 0..cells.len() {
        if seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let mut member = Vec::new();
        while let Some(current) = stack.pop() {
            member.push(current);
            let cell = &cells[current];
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let key = (cell.ix + dx, cell.iy + dy);
                if let Some(neighbors) = index.get(&key) {
                    for &next in neighbors {
                        if seen[next] {
                            continue;
                        }
                        if (cells[next].z - cell.z).abs() > params.cell_mm * 1.5 {
                            continue;
                        }
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        if let Some(component) = classify(&cells, &member, &solids, &params, origin_x, origin_y) {
            out.push(component);
        }
    }
    out
}

fn floor_under(solids: &HashMap<(i32, i32), Vec<(f64, f64)>>, ix: i32, iy: i32, z: f64) -> f64 {
    // The column's own intervals are recorded before the underside is pushed,
    // so a shelf below this face is visible. Intervals that contain z are the
    // solid we just entered and are ignored.
    let Some(intervals) = solids.get(&(ix, iy)) else {
        return 0.0;
    };
    let mut floor = 0.0f64;
    for (_start, end) in intervals {
        if *end < z - 0.3 {
            floor = floor.max(*end);
        }
    }
    floor
}

fn classify(
    cells: &[Cell],
    member: &[usize],
    solids: &HashMap<(i32, i32), Vec<(f64, f64)>>,
    params: &SupportParams,
    origin_x: f64,
    origin_y: f64,
) -> Option<Component> {
    let group: Vec<&Cell> = member.iter().map(|id| &cells[*id]).collect();
    let area = group.len() as f64 * params.cell_mm * params.cell_mm;
    let min_ix = group.iter().map(|cell| cell.ix).min()?;
    let max_ix = group.iter().map(|cell| cell.ix).max()?;
    let min_iy = group.iter().map(|cell| cell.iy).min()?;
    let max_iy = group.iter().map(|cell| cell.iy).max()?;
    let along_x = (max_ix - min_ix) >= (max_iy - min_iy);
    let length = if along_x {
        (max_ix - min_ix + 1) as f64 * params.cell_mm
    } else {
        (max_iy - min_iy + 1) as f64 * params.cell_mm
    };
    let width = if along_x {
        (max_iy - min_iy + 1) as f64 * params.cell_mm
    } else {
        (max_ix - min_ix + 1) as f64 * params.cell_mm
    };
    let inside: HashSet<(i32, i32)> = group.iter().map(|cell| (cell.ix, cell.iy)).collect();
    let anchored = |cell: &Cell, dx: i32, dy: i32| -> bool {
        let key = (cell.ix + dx, cell.iy + dy);
        if inside.contains(&key) {
            return false;
        }
        solid_below(solids.get(&key), cell.z)
    };
    let end = |extreme: i32, horizontal: bool, negative: bool| -> bool {
        group
            .iter()
            .filter(|cell| {
                if horizontal {
                    cell.ix == extreme
                } else {
                    cell.iy == extreme
                }
            })
            .any(|cell| {
                if horizontal {
                    anchored(cell, if negative { -1 } else { 1 }, 0)
                } else {
                    anchored(cell, 0, if negative { -1 } else { 1 })
                }
            })
    };
    let (low, high) = if along_x {
        (end(min_ix, true, true), end(max_ix, true, false))
    } else {
        (end(min_iy, false, true), end(max_iy, false, false))
    };
    let any_anchor = group.iter().any(|cell| {
        anchored(cell, 1, 0)
            || anchored(cell, -1, 0)
            || anchored(cell, 0, 1)
            || anchored(cell, 0, -1)
    });
    let anchor_ends = u8::from(low) + u8::from(high);
    let mut kind = if low && high && length <= params.max_bridge_mm + params.cell_mm * 0.5 {
        FeatureKind::Bridge
    } else if !any_anchor {
        FeatureKind::Island
    } else if anchor_ends == 1 {
        FeatureKind::Cantilever
    } else {
        FeatureKind::Overhang
    };
    let angle = group.iter().map(|cell| cell.angle).sum::<f64>() / group.len() as f64;
    if matches!(kind, FeatureKind::Cantilever | FeatureKind::Overhang)
        && angle <= params.overhang_angle_deg + params.cosmetic_band_deg
        && area >= params.cosmetic_min_area_mm2
    {
        kind = FeatureKind::Cosmetic;
    }
    let z = group.iter().map(|cell| cell.z).sum::<f64>() / group.len() as f64;
    let samples = group
        .iter()
        .map(|cell| SurfaceSample {
            x: origin_x + (cell.ix as f64 + 0.5) * params.cell_mm,
            y: origin_y + (cell.iy as f64 + 0.5) * params.cell_mm,
            z: cell.z,
            angle_from_vertical_deg: cell.angle,
        })
        .collect();
    let rects = rectangles(&group, params.cell_mm, origin_x, origin_y);
    Some(Component {
        span: Span {
            kind,
            length_mm: length,
            width_mm: width,
            area_mm2: area,
            z_mm: z,
            angle_from_vertical_deg: angle,
            anchor_ends,
            samples,
        },
        rects,
    })
}

fn solid_below(intervals: Option<&Vec<(f64, f64)>>, z: f64) -> bool {
    let Some(intervals) = intervals else {
        return false;
    };
    intervals
        .iter()
        .any(|(start, end)| *start < z - 0.3 && *end > z - 0.8)
}

fn rectangles(group: &[&Cell], cell: f64, origin_x: f64, origin_y: f64) -> Vec<Rect> {
    let mut rows: BTreeMap<i32, Vec<&Cell>> = BTreeMap::new();
    for cell in group {
        rows.entry(cell.iy).or_default().push(*cell);
    }
    let mut runs = Vec::new();
    for (iy, mut row) in rows {
        row.sort_by_key(|cell| cell.ix);
        let mut ix0 = row[0].ix;
        let mut prev = row[0].ix;
        let mut z = row[0].z;
        let mut floor = row[0].floor;
        for item in row.iter().skip(1) {
            let same = item.ix == prev + 1 && (item.floor - floor).abs() < 1.0;
            if same {
                prev = item.ix;
                z = z.min(item.z);
                floor = floor.max(item.floor);
            } else {
                runs.push(Run {
                    iy,
                    ix0,
                    ix1: prev,
                    z,
                    floor,
                });
                ix0 = item.ix;
                prev = item.ix;
                z = item.z;
                floor = item.floor;
            }
        }
        runs.push(Run {
            iy,
            ix0,
            ix1: prev,
            z,
            floor,
        });
    }
    runs.sort_by_key(|run| (run.ix0, run.ix1, run.iy));
    let mut rects = Vec::new();
    let mut current: Option<(Run, i32)> = None;
    for run in runs {
        if let Some((active, y0)) = current.as_mut() {
            let extend = active.ix0 == run.ix0
                && active.ix1 == run.ix1
                && run.iy == active.iy + 1
                && (run.floor - active.floor).abs() < 1.0;
            if extend {
                active.iy = run.iy;
                active.z = active.z.min(run.z);
                active.floor = active.floor.max(run.floor);
                continue;
            }
            rects.push(rect_from(active, *y0, cell, origin_x, origin_y));
        }
        let y0 = run.iy;
        current = Some((run, y0));
    }
    if let Some((active, y0)) = current {
        rects.push(rect_from(&active, y0, cell, origin_x, origin_y));
    }
    rects
}

fn rect_from(run: &Run, y0: i32, cell: f64, origin_x: f64, origin_y: f64) -> Rect {
    Rect {
        x0: origin_x + run.ix0 as f64 * cell,
        x1: origin_x + (run.ix1 as f64 + 1.0) * cell,
        y0: origin_y + y0 as f64 * cell,
        y1: origin_y + (run.iy as f64 + 1.0) * cell,
        z_top: run.z,
        z_floor: run.floor,
    }
}

fn everywhere_totals(found: &[Component], params: &SupportParams) -> (f64, f64) {
    let mut volume = 0.0;
    let mut contact = 0.0;
    for component in found {
        if component.span.kind == FeatureKind::Cosmetic
            && component.span.angle_from_vertical_deg
                <= params.overhang_angle_deg + params.cosmetic_band_deg
        {
            // Everywhere still supports a shallow face. Cosmetic is only a
            // smart-support exception, so it counts here too.
        }
        for rect in &component.rects {
            let top = rect.z_top - 0.2;
            let bottom = if rect.z_floor > 0.05 {
                rect.z_floor + 0.2
            } else {
                0.0
            };
            if top - bottom < 0.4 {
                continue;
            }
            let area = (rect.x1 - rect.x0) * (rect.y1 - rect.y0);
            volume += area * (top - bottom);
            contact += area;
        }
    }
    (volume, contact)
}

fn column(mesh: &TriangleMesh, x: f64, y: f64) -> (Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let mut hits: Vec<(f64, i32, f64)> = Vec::new();
    for triangle in &mesh.triangles {
        let Some(corners) = triangle_corners(mesh, *triangle) else {
            continue;
        };
        let Some((z, normal_z)) = pierce(x, y, corners) else {
            continue;
        };
        let enter = if normal_z < 0.0 { 1 } else { -1 };
        hits.push((z, enter, angle_from_vertical(normal_z)));
    }
    hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut intervals = Vec::new();
    let mut undersides = Vec::new();
    let mut cursor = 0;
    let mut counter = 0i32;
    let mut solid_from: Option<f64> = None;
    while cursor < hits.len() {
        let z = hits[cursor].0;
        let mut delta = 0;
        let mut entered = false;
        let mut angle = 0.0;
        while cursor < hits.len() && (hits[cursor].0 - z).abs() < 1e-3 {
            delta += hits[cursor].1;
            if hits[cursor].1 > 0 {
                entered = true;
                angle = hits[cursor].2;
            }
            cursor += 1;
        }
        let before = counter;
        counter += delta;
        if before <= 0 && counter > 0 {
            if entered {
                undersides.push((z, angle));
            }
            solid_from = Some(z);
        } else if before > 0 && counter <= 0 {
            if let Some(start) = solid_from.take() {
                intervals.push((start, z));
            }
        }
    }
    (intervals, undersides)
}

fn triangle_corners(mesh: &TriangleMesh, triangle: [u32; 3]) -> Option<[[f64; 3]; 3]> {
    let mut corners = [[0.0; 3]; 3];
    for (slot, index) in triangle.iter().enumerate() {
        let vertex = mesh.vertices.get(*index as usize)?;
        corners[slot] = [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64];
    }
    Some(corners)
}

fn pierce(x: f64, y: f64, corners: [[f64; 3]; 3]) -> Option<(f64, f64)> {
    let [a, b, c] = corners;
    let normal_z = {
        let ux = b[0] - a[0];
        let uy = b[1] - a[1];
        let uz = b[2] - a[2];
        let vx = c[0] - a[0];
        let vy = c[1] - a[1];
        let vz = c[2] - a[2];
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        if len < 1e-8 || nz.abs() / len < 1e-3 {
            return None;
        }
        nz / len
    };
    let denom = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if denom.abs() < 1e-12 {
        return None;
    }
    let w0 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / denom;
    let w1 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / denom;
    let w2 = 1.0 - w0 - w1;
    if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
        return None;
    }
    let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
    Some((z, normal_z))
}

fn box_positions(min: [f64; 3], max: [f64; 3]) -> Vec<f32> {
    let mesh = region_mesh(&SupportRegion {
        id: 0,
        kind: FeatureKind::Overhang,
        role: RegionRole::Enforce,
        enabled: true,
        min,
        max,
        contact_area_mm2: 0.0,
        volume_mm3: 0.0,
        summary: String::new(),
    });
    let mut positions = Vec::with_capacity(mesh.triangles.len() * 9);
    for triangle in &mesh.triangles {
        for index in triangle {
            if let Some(vertex) = mesh.vertices.get(*index as usize) {
                positions.extend_from_slice(vertex);
            }
        }
    }
    positions
}

pub(crate) fn append_box(mesh: &mut TriangleMesh, min: [f32; 3], max: [f32; 3]) {
    let base = mesh.vertices.len() as u32;
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    mesh.vertices.extend([
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ]);
    let faces: [[u32; 3]; 12] = [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    for face in faces {
        mesh.triangles
            .push([base + face[0], base + face[1], base + face[2]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes(parts: &[[f32; 6]]) -> TriangleMesh {
        let mut mesh = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        for part in parts {
            append_box(
                &mut mesh,
                [part[0], part[1], part[2]],
                [part[3], part[4], part[5]],
            );
        }
        mesh
    }

    fn t_shape() -> TriangleMesh {
        boxes(&[
            [15.0, 0.0, 0.0, 25.0, 10.0, 20.0],
            [0.0, 0.0, 20.0, 40.0, 10.0, 24.0],
        ])
    }

    fn bridge() -> TriangleMesh {
        boxes(&[
            [0.0, 0.0, 0.0, 8.0, 8.0, 20.0],
            [24.0, 0.0, 0.0, 32.0, 8.0, 20.0],
            [8.0, 0.0, 20.0, 24.0, 8.0, 24.0],
        ])
    }

    fn mushroom() -> TriangleMesh {
        boxes(&[
            [10.0, 10.0, 0.0, 20.0, 20.0, 20.0],
            [0.0, 0.0, 20.0, 30.0, 30.0, 24.0],
        ])
    }

    fn enforcers(plan: &SupportPlan) -> Vec<&SupportRegion> {
        plan.regions
            .iter()
            .filter(|region| region.role == RegionRole::Enforce && region.enabled)
            .collect()
    }

    fn covers(region: &SupportRegion, x: f64, y: f64, z: f64) -> bool {
        x >= region.min[0] - 0.05
            && x <= region.max[0] + 0.05
            && y >= region.min[1] - 0.05
            && y <= region.max[1] + 0.05
            && z >= region.min[2] - 0.05
            && z <= region.max[2] + 0.05
    }

    #[test]
    fn horizontal_face_is_a_full_overhang() {
        assert!((angle_from_vertical(-1.0) - 90.0).abs() < 1e-6);
        assert!(angle_from_vertical(0.0) < 1e-6);
        let shallow = angle_from_vertical(-0.8192);
        assert!(shallow > 50.0 && shallow < 60.0, "{shallow}");
    }

    #[test]
    fn t_shape_supports_the_wings_and_not_the_stem() {
        let plan = propose(&t_shape(), &SupportParams::default());
        assert_eq!(
            plan.spans
                .iter()
                .filter(|span| span.kind == FeatureKind::Cantilever)
                .count(),
            2
        );
        assert!(plan
            .spans
            .iter()
            .all(|span| span.kind != FeatureKind::Bridge));
        let columns = enforcers(&plan);
        assert!(
            columns.iter().any(|region| covers(region, 7.0, 5.0, 10.0)),
            "{columns:?}"
        );
        assert!(columns.iter().any(|region| covers(region, 33.0, 5.0, 10.0)));
        assert!(columns
            .iter()
            .all(|region| !covers(region, 20.0, 5.0, 10.0)));
        assert!(plan.comparison.proposed_volume_mm3 > 100.0);
    }

    #[test]
    fn bridge_is_blocked_until_the_span_is_too_long() {
        let plan = propose(&bridge(), &SupportParams::default());
        assert!(
            plan.spans
                .iter()
                .any(|span| span.kind == FeatureKind::Bridge),
            "{:?}",
            plan.spans.iter().map(|span| span.kind).collect::<Vec<_>>()
        );
        assert!(
            enforcers(&plan).is_empty(),
            "a bridge should not grow a column"
        );
        assert!(plan
            .regions
            .iter()
            .any(|region| { region.role == RegionRole::Block && covers(region, 16.0, 4.0, 20.0) }));
        assert!(plan.comparison.everywhere_volume_mm3 > plan.comparison.proposed_volume_mm3 + 50.0);

        let mut short = SupportParams::default();
        short.max_bridge_mm = 5.0;
        let forced = propose(&bridge(), &short);
        assert!(forced.spans.iter().any(|span| span.kind.wants_support()));
        assert!(!enforcers(&forced).is_empty());
    }

    #[test]
    fn mushroom_supports_the_cap_and_leaves_the_stem() {
        let plan = propose(&mushroom(), &SupportParams::default());
        assert!(plan.spans.iter().any(|span| {
            span.kind == FeatureKind::Cantilever || span.kind == FeatureKind::Overhang
        }));
        assert!(plan
            .spans
            .iter()
            .all(|span| span.kind != FeatureKind::Bridge));
        let columns = enforcers(&plan);
        assert!(
            columns.iter().any(|region| covers(region, 2.0, 2.0, 10.0)),
            "{columns:?}"
        );
        assert!(columns
            .iter()
            .all(|region| !covers(region, 15.0, 15.0, 10.0)));
    }

    #[test]
    fn floating_box_is_an_island() {
        let mesh = boxes(&[[0.0, 0.0, 12.0, 8.0, 8.0, 16.0]]);
        let plan = propose(&mesh, &SupportParams::default());
        assert_eq!(plan.spans.len(), 1);
        assert_eq!(plan.spans[0].kind, FeatureKind::Island);
        assert_eq!(plan.spans[0].anchor_ends, 0);
        let column = &enforcers(&plan)[0];
        assert!(
            column.min[2] < 1.0,
            "the island column should reach the bed"
        );
        assert!(column.max[2] > 10.0);
    }

    #[test]
    fn engine_uses_support_meshes_for_a_bridge() {
        let Ok(program) = std::env::var("SHALE_CURAENGINE") else {
            return;
        };
        let Ok(cura) = std::env::var("SHALE_CURA_DIR") else {
            return;
        };
        let Ok(materials) = std::env::var("SHALE_MATERIALS_DIR") else {
            return;
        };
        let catalog = crate::Catalog::open(crate::ResourcePaths {
            cura: cura.into(),
            materials: materials.into(),
        })
        .expect("catalog");
        let engine = crate::CliEngine {
            program: program.into(),
            lib_dir: std::env::var("SHALE_CURAENGINE_LIB_DIR")
                .ok()
                .map(Into::into),
        };
        let mesh = bridge();
        let plan = propose(&mesh, &SupportParams::default());
        assert!(plan
            .spans
            .iter()
            .any(|span| span.kind == FeatureKind::Bridge));
        let settings = crate::UserSettings {
            layer_height: Some(0.2),
            infill_sparse_density: Some(10.0),
            support_enable: Some(false),
            material_print_temperature: Some(200.0),
            material_bed_temperature: Some(60.0),
            extra: Vec::new(),
        };
        let placed = [crate::PlacedModel {
            name: "bridge".into(),
            mesh,
            transform: crate::Transform::default(),
            settings: Vec::new(),
            slot: None,
        }];
        let comparison = crate::compare_support(
            &catalog,
            &engine,
            "creality_ender3",
            Some("0.4mm Nozzle".into()),
            Some("draft".into()),
            Some("generic_pla".into()),
            settings,
            &placed,
            &plan,
            &mut |_| {},
        )
        .expect("compare slices");
        let smart = comparison.proposed_support_metres.unwrap_or(1.0);
        let everywhere = comparison.everywhere_support_metres.unwrap_or(0.0);
        assert!(
            everywhere > smart + 0.05,
            "everywhere {everywhere} m should use more support filament than the bridge plan {smart} m\n{comparison:?}"
        );
        assert!(comparison.times_from_engine);
    }

    #[test]
    fn a_shallow_attached_slope_is_cosmetic() {
        // Wall, then a plate whose underside is about 55° from vertical.
        let mesh = boxes(&[[0.0, 0.0, 0.0, 4.0, 10.0, 20.0]]);
        let mut sloped = mesh;
        append_slope(&mut sloped, 4.0, 24.0, 0.0, 10.0, 20.0, 6.0, 3.0);
        let plan = propose(&sloped, &SupportParams::default());
        assert!(
            plan.spans
                .iter()
                .any(|span| span.kind == FeatureKind::Cosmetic),
            "{:?}",
            plan.spans
                .iter()
                .map(|span| (span.kind, span.angle_from_vertical_deg, span.area_mm2))
                .collect::<Vec<_>>()
        );
        assert!(enforcers(&plan).is_empty());
        assert!(plan.comparison.everywhere_volume_mm3 > 0.0);
    }

    /// Underside from (x0, z0) to (x1, z1), thickness `thick` straight up.
    fn append_slope(
        mesh: &mut TriangleMesh,
        x0: f32,
        x1: f32,
        y0: f32,
        y1: f32,
        z0: f32,
        z1: f32,
        thick: f32,
    ) {
        let base = mesh.vertices.len() as u32;
        // Downward normal: wind the underside so nz is negative.
        mesh.vertices.extend([
            [x0, y0, z0],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z0],
            [x0, y0, z0 + thick],
            [x1, y0, z1 + thick],
            [x1, y1, z1 + thick],
            [x0, y1, z0 + thick],
        ]);
        let faces: [[u32; 3]; 12] = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 4, 5],
            [0, 5, 1],
            [1, 5, 6],
            [1, 6, 2],
            [2, 6, 7],
            [2, 7, 3],
            [3, 7, 4],
            [3, 4, 0],
        ];
        for face in faces {
            mesh.triangles
                .push([base + face[0], base + face[1], base + face[2]]);
        }
    }
}

#[cfg(test)]
mod modifier_role_tests {
    use super::*;

    #[test]
    fn the_two_roles_are_two_different_asks() {
        // The engine is told what a painted volume is for through the meshes it
        // is handed: a support block, or a slab that may not be supported.
        let enforce = modifier_settings(RegionRole::Enforce);
        let block = modifier_settings(RegionRole::Block);
        let has = |pairs: &[(String, String)], key: &str| {
            pairs.iter().any(|(name, value)| name == key && value == "true")
        };
        assert!(has(&enforce, "support_mesh"));
        assert!(has(&block, "anti_overhang_mesh"));
        assert!(!has(&enforce, "anti_overhang_mesh"));
        assert!(!has(&block, "support_mesh"));
        assert_ne!(enforce, block);
    }
}
