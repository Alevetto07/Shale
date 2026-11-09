//! Cutting a solid with a plane.
//!
//! Bambu Studio's *Cut* tool slices a part in two and closes both halves; this
//! does the same thing on the mesh. The hard part is not the cut, it is the
//! cap: a clipping routine alone leaves two open shells, and an engine that
//! fills by winding sees an open shell as a void and prints nothing (or prints
//! the inside). So every triangle the plane crosses contributes its cut
//! segment, the segments are stitched into closed loops, and each loop is
//! triangulated and welded onto its side with the winding that faces outward.
//!
//! Everything here is geometry in the part's own frame: no engine, no transforms.

use crate::mesh::TriangleMesh;
use crate::csg;

/// Distances are in millimetres; this is a tenth of a micron.
const EPS: f64 = 1e-6;

#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub point: [f64; 3],
    pub normal: [f64; 3],
}

impl Plane {
    pub fn new(point: [f64; 3], normal: [f64; 3]) -> Self {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        let normal = if length > 0.0 {
            [normal[0] / length, normal[1] / length, normal[2] / length]
        } else {
            [0.0, 0.0, 1.0]
        };
        Self { point, normal }
    }

    pub fn distance(&self, point: [f64; 3]) -> f64 {
        (point[0] - self.point[0]) * self.normal[0]
            + (point[1] - self.point[1]) * self.normal[1]
            + (point[2] - self.point[2]) * self.normal[2]
    }
}

pub struct Halves {
    /// The side the normal points towards.
    pub above: TriangleMesh,
    /// The side the normal points away from.
    pub below: TriangleMesh,
}

impl Halves {
    fn empty() -> Self {
        Self {
            above: TriangleMesh {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
            below: TriangleMesh {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        self.above.triangles.is_empty() || self.below.triangles.is_empty()
    }
}

/// Split a closed mesh along a plane, capping both halves.
pub fn cut(mesh: &TriangleMesh, plane: &Plane) -> Halves {
    let mut halves = Halves::empty();
    let mut segments: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for triangle in &mesh.triangles {
        let corners = [
            as_f64(mesh.vertices[triangle[0] as usize]),
            as_f64(mesh.vertices[triangle[1] as usize]),
            as_f64(mesh.vertices[triangle[2] as usize]),
        ];
        let distance = [
            plane.distance(corners[0]),
            plane.distance(corners[1]),
            plane.distance(corners[2]),
        ];
        if distance.iter().all(|d| *d >= -EPS) {
            push_triangle(&mut halves.above, &corners);
            continue;
        }
        if distance.iter().all(|d| *d <= EPS) {
            push_triangle(&mut halves.below, &corners);
            continue;
        }
        // The plane crosses this triangle: each side keeps a polygon, and the
        // place the plane met the triangle becomes a cap segment.
        let above = clip(&corners, &distance, true);
        let below = clip(&corners, &distance, false);
        fan(&mut halves.above, &above);
        fan(&mut halves.below, &below);
        if let Some(segment) = crossing(corners, distance) {
            segments.push(segment);
        }
    }
    // Cap each half. The plane's normal is the outward direction for `above`,
    // so its cap faces -normal: the loop is wound the other way round.
    for polygon in loops(&segments) {
        let cap = triangulate_cap(&polygon, plane);
        for triangle in cap {
            let points = [polygon[triangle[0]], polygon[triangle[1]], polygon[triangle[2]]];
            push_triangle(&mut halves.below, &points);
            push_triangle(&mut halves.above, &[points[0], points[2], points[1]]);
        }
    }
    halves
}

/// What a cut leaves behind so the two halves can be put back together: round
/// pins on the lower half and matching holes in the upper.
#[derive(Clone, Copy, Debug)]
pub struct Pin {
    /// Pin radius, in millimetres.
    pub radius: f64,
    /// How far the pin stands off the cut face.
    pub height: f64,
    /// Radial play, and the same amount again in depth so a pin cannot bottom
    /// out. Bambu calls this the clearance.
    pub clearance: f64,
}

impl Default for Pin {
    fn default() -> Self {
        Self {
            radius: 2.0,
            height: 5.0,
            clearance: 0.2,
        }
    }
}

/// Cut, and put pins on the lower half with holes in the upper.
///
/// The pins point along the cut normal — up out of the lower half — because a
/// vertical cylinder prints without support; a pin hanging down from the upper
/// half would need it. `count` is how many pins to try for: they are placed
/// inside the cut face and only where the whole pin fits, so a narrow cross
/// section gets fewer than it asked for rather than a pin in mid-air.
pub fn cut_with_pins(mesh: &TriangleMesh, plane: &Plane, pin: Pin, count: usize) -> Halves {
    let mut halves = cut(mesh, plane);
    if halves.is_empty() || pin.radius <= 0.0 {
        return halves;
    }
    let segments = cross_section(mesh, plane);
    let rings = loops(&segments);
    let centres = pin_centres(&rings, plane, pin, count);
    let upright = normalise(plane.normal);
    for centre in centres {
        // The pin stands proud of the cut face on the lower half…
        let peg = csg::cylinder(
            centre,
            upright,
            pin.radius,
            pin.height,
            32,
        );
        halves.below = csg::union(&halves.below, &peg);
        // …and the upper half gets a hole that is wider by the clearance and
        // deeper by the same amount, so the pin has somewhere to go.
        let mouth = add(centre, scale(upright, -pin.clearance));
        let bore = csg::cylinder(
            mouth,
            upright,
            pin.radius + pin.clearance,
            pin.height + pin.clearance * 2.0,
            32,
        );
        halves.above = csg::subtract(&halves.above, &bore);
    }
    halves
}

/// A dovetail: a tab that is wider at its tip than at its root.
///
/// Pins stop the halves sliding across the seam; they do not stop the seam
/// opening, because a cylinder pulls straight out of its own hole. A dovetail
/// does: the slot it sits in is narrower than its far end, so the two halves
/// can be pressed together and then hold. Bambu's ear does the same job with a
/// pair of hooks; this is the same idea in one piece.
#[derive(Clone, Copy, Debug)]
pub struct Dovetail {
    /// Width where the tab leaves the cut face, in millimetres.
    pub root: f64,
    /// Width at the far end. Must be larger than `root`, or it is only a peg.
    pub tip: f64,
    /// How far the tab stands proud of the cut face.
    pub depth: f64,
    /// How thick the tab is across the face.
    pub thickness: f64,
    /// Play on every side, so the two halves assemble after printing.
    pub clearance: f64,
}

impl Dovetail {
    /// A tab shaped like its own root width: twice as wide at the tip, as deep
    /// as it is wide, and as thick again. One number for the person cutting.
    pub fn from_root(root: f64) -> Self {
        Self {
            root,
            tip: root * 2.0,
            depth: root,
            thickness: root,
            clearance: 0.2,
        }
    }
}

impl Default for Dovetail {
    fn default() -> Self {
        Self::from_root(6.0)
    }
}

/// Cut, and put one dovetail across the seam: the tab on the lower half, the
/// slot in the upper.
///
/// The tab is placed in the middle of the largest cross-section and only if the
/// whole of it — tip included, plus clearance — lands inside that face. A part
/// too narrow to hold one is cut plain, because a tab hanging over an edge is
/// not a joint, it is a burr.
/// A cut that may or may not have carried a tab.
///
/// `tab_mm3` is `None` when the face had no room for one, which is a different
/// answer from "the tab was this big" — the caller has to be able to say which
/// happened, because a control that silently does nothing is a control that
/// lies.
pub struct JointCut {
    pub halves: Halves,
    pub tab_mm3: Option<f64>,
}

pub fn cut_with_dovetail(mesh: &TriangleMesh, plane: &Plane, dovetail: Dovetail) -> JointCut {
    let halves = cut(mesh, plane);
    let plain = JointCut {
        halves,
        tab_mm3: None,
    };
    if plain.halves.is_empty() || dovetail.root <= 0.0 || dovetail.tip <= dovetail.root {
        return plain;
    }
    let mut halves = plain.halves;
    let segments = cross_section(mesh, plane);
    let rings = loops(&segments);
    let up = normalise(plane.normal);

    // The face with the most room on it, whichever way the plane's own axes
    // happen to point.
    let mut biggest: Option<(&Vec<[f64; 3]>, [f64; 3], [f64; 3])> = None;
    let mut largest = 0.0;
    for (u, v) in [
        (plane_u(plane.normal), plane_v(plane.normal)),
        (plane_v(plane.normal), plane_u(plane.normal)),
    ] {
        for ring in rings.iter().filter(|ring| ring.len() >= 3) {
            let polygon: Vec<[f64; 2]> = ring.iter().map(|point| [dot(*point, u), dot(*point, v)]).collect();
            let area = signed_area(&polygon).abs();
            if area > largest {
                largest = area;
                biggest = Some((ring, u, v));
            }
        }
    }
    let Some((ring, u, v)) = biggest else {
        return JointCut { halves, tab_mm3: None };
    };
    let flat = |point: [f64; 3]| [dot(point, u), dot(point, v)];
    let polygon: Vec<[f64; 2]> = ring.iter().map(|point| flat(*point)).collect();
    let centre = scale(
        ring.iter().fold([0.0f64; 3], |sum, point| add_3(sum, *point)),
        1.0 / ring.len() as f64,
    );
    let middle = flat(centre);

    // A tab is a rectangle, not a disc: it has to fit across the face, not
    // reach equally far in every direction. Asking for a circle the width of
    // the tip is how a perfectly good arm-shaped face gets refused its tab.
    //
    // So the tab is fitted to the face it is cut into — but only down to half
    // the size that was asked for. A tab a third of the width of the one you
    // dialled in is not the joint you asked for, and the cut says nothing about
    // having quietly shrunk it; past that point it is better to cut plain.
    let margin = dovetail.clearance + 0.4;
    let spread = |axis: usize| {
        let low = polygon.iter().map(|p| p[axis]).fold(f64::MAX, f64::min);
        let high = polygon.iter().map(|p| p[axis]).fold(f64::MIN, f64::max);
        high - low
    };
    let rect_fits = |half_u: f64, half_v: f64| {
        inside(&polygon, middle)
            && [
                [-1.0, -1.0],
                [1.0, -1.0],
                [1.0, 1.0],
                [-1.0, 1.0],
            ]
            .iter()
            .all(|corner| {
                inside(
                    &polygon,
                    [
                        middle[0] + corner[0] * (half_u + margin),
                        middle[1] + corner[1] * (half_v + margin),
                    ],
                )
            })
    };

    let wanted = (dovetail.root, dovetail.tip, dovetail.thickness);
    let room = (spread(0) - margin * 2.0, spread(1) - margin * 2.0);
    let mut chosen: Option<(f64, f64, f64)> = None;
    for shrink in [1.0, 0.8, 0.6] {
        let tip = (dovetail.tip * shrink).min(room.0);
        let thickness = (dovetail.thickness * shrink).min(room.1);
        let root = (dovetail.root * shrink).min(tip * 0.5).max(1.5);
        if tip < wanted.1 * 0.5 || thickness < wanted.2 * 0.5 {
            break;
        }
        if rect_fits(tip * 0.5, thickness * 0.5) {
            chosen = Some((root, tip, thickness));
            break;
        }
    }
    let Some((root, tip, thickness)) = chosen else {
        return JointCut { halves, tab_mm3: None };
    };

    // A tab that comes out of the far side of the other half is a spike, not a
    // joint: keep it a millimetre short of the surface it is heading for.
    let (_, above_bounds) = halves.above.bounds();
    let face = dot(plane.point, up);
    let top = [
        above_bounds[0] as f64,
        above_bounds[1] as f64,
        above_bounds[2] as f64,
    ];
    let headroom = dot(top, up) - face - 1.0;
    let depth = dovetail.depth.min(headroom.max(1.0)).max(1.0);

    let tab = dovetail_prism(centre, u, v, up, root, tip, thickness, depth);
    halves.below = csg::union(&halves.below, &tab);

    // The slot is the same shape, grown by the clearance on every side and
    // started a hair behind the cut face so the two booleans do not share a
    // plane.
    let mouth = add(centre, scale(up, -dovetail.clearance));
    let slot = dovetail_prism(
        mouth,
        u,
        v,
        up,
        root + dovetail.clearance * 2.0,
        tip + dovetail.clearance * 2.0,
        thickness + dovetail.clearance * 2.0,
        depth + dovetail.clearance * 2.0,
    );
    halves.above = csg::subtract(&halves.above, &slot);
    JointCut {
        halves,
        tab_mm3: Some(volume(&tab)),
    }
}

/// A trapezoid prism: `root` wide where it meets the face, `tip` wide at the
/// far end, `thickness` across, standing `depth` out along `up`.
fn dovetail_prism(
    centre: [f64; 3],
    u: [f64; 3],
    v: [f64; 3],
    up: [f64; 3],
    root: f64,
    tip: f64,
    thickness: f64,
    depth: f64,
) -> TriangleMesh {
    let at = |across: f64, out: f64, along: f64| {
        add(
            centre,
            add(scale(u, across), add(scale(v, along), scale(up, out))),
        )
    };
    let (r, t) = (root * 0.5, thickness * 0.5);
    let (q, s) = (tip * 0.5, thickness * 0.5);
    let corners = [
        at(-r, 0.0, -t),
        at(r, 0.0, -t),
        at(r, 0.0, t),
        at(-r, 0.0, t),
        at(-q, depth, -s),
        at(q, depth, -s),
        at(q, depth, s),
        at(-q, depth, s),
    ];
    let middle = scale(
        corners.iter().fold([0.0f64; 3], |sum, point| add_3(sum, *point)),
        1.0 / 8.0,
    );
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    // Corners run 0..3 round the face and 4..7 round the tip, so the six faces
    // pair up as: back, front, two sides at constant v, two flanks at constant
    // u. Pairing a face corner with the tip corner two along twists the flank
    // into a non-planar quad, and a quad split into two triangles then holds a
    // different volume from the solid it is supposed to be.
    let quads = [
        [0usize, 1, 2, 3], // back, against the cut face
        [4, 5, 6, 7],      // front, the far end
        [0, 1, 5, 4],      // side, at -v
        [3, 2, 6, 7],      // side, at +v
        [0, 4, 7, 3],      // flank, at -u
        [1, 5, 6, 2],      // flank, at +u
    ];
    for quad in quads {
        push_quad(&mut mesh, &[
            corners[quad[0]],
            corners[quad[1]],
            corners[quad[2]],
            corners[quad[3]],
        ], middle);
    }
    mesh
}

/// Two triangles for a quad, wound so the face points away from `inside`.
fn push_quad(mesh: &mut TriangleMesh, corners: &[[f64; 3]; 4], inside: [f64; 3]) {
    let normal = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
    let outward = sub(
        scale(
            corners.iter().fold([0.0f64; 3], |sum, point| add_3(sum, *point)),
            0.25,
        ),
        inside,
    );
    if dot(normal, outward) < 0.0 {
        push_triangle(mesh, &[corners[0], corners[2], corners[1]]);
        push_triangle(mesh, &[corners[0], corners[3], corners[2]]);
    } else {
        push_triangle(mesh, &[corners[0], corners[1], corners[2]]);
        push_triangle(mesh, &[corners[0], corners[2], corners[3]]);
    }
}

/// Every place the plane meets an edge, as a segment on the plane.
fn cross_section(mesh: &TriangleMesh, plane: &Plane) -> Vec<([f64; 3], [f64; 3])> {
    let mut segments = Vec::new();
    for triangle in &mesh.triangles {
        let corners = [
            as_f64(mesh.vertices[triangle[0] as usize]),
            as_f64(mesh.vertices[triangle[1] as usize]),
            as_f64(mesh.vertices[triangle[2] as usize]),
        ];
        let distance = [
            plane.distance(corners[0]),
            plane.distance(corners[1]),
            plane.distance(corners[2]),
        ];
        if distance.iter().all(|d| *d > EPS) || distance.iter().all(|d| *d < -EPS) {
            continue;
        }
        if let Some(segment) = crossing(corners, distance) {
            segments.push(segment);
        }
    }
    segments
}

/// Points inside the cut face where a pin of this size fits, spread out.
fn pin_centres(rings: &[Vec<[f64; 3]>], plane: &Plane, pin: Pin, count: usize) -> Vec<[f64; 3]> {
    let (u, v) = (plane_u(plane.normal), plane_v(plane.normal));
    let flat = |point: [f64; 3]| [dot(point, u), dot(point, v)];
    let reach = pin.radius + pin.clearance + 0.6;
    // A pin needs clearance from the wall, and two pins need clearance from each
    // other.
    let spacing = reach * 2.0;
    let mut candidates: Vec<[f64; 3]> = Vec::new();
    let mut rings_sorted: Vec<&Vec<[f64; 3]>> = rings.iter().filter(|ring| ring.len() >= 3).collect();
    rings_sorted.sort_by(|a, b| b.len().cmp(&a.len()));
    for ring in rings_sorted {
        let polygon: Vec<[f64; 2]> = ring.iter().map(|point| flat(*point)).collect();
        let centre = ring
            .iter()
            .fold([0.0f64; 3], |sum, point| add_3(sum, *point));
        let centre = scale(centre, 1.0 / ring.len() as f64);
        let mut here = vec![centre];
        // Along the way out from the middle, halfway to each corner: a wide
        // face gets pins spread across it, a narrow one keeps the middle.
        for point in ring.iter().step_by((ring.len() / 8).max(1)) {
            here.push(lerp(centre, *point, 0.55));
        }
        for point in here {
            let middle = flat(point);
            if fits(&polygon, middle, reach) {
                candidates.push([point[0], point[1], point[2]]);
            }
        }
    }
    let mut chosen: Vec<[f64; 3]> = Vec::new();
    // The middle of the biggest face first, then whatever is far enough away.
    candidates.sort_by(|a, b| {
        let da = distance_to(*a, &rings, plane);
        let db = distance_to(*b, &rings, plane);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    for candidate in candidates {
        if chosen.len() >= count.max(1) {
            break;
        }
        if chosen
            .iter()
            .all(|existing| length(sub(candidate, *existing)) >= spacing)
        {
            chosen.push(candidate);
        }
    }
    chosen
}

fn distance_to(point: [f64; 3], _rings: &[Vec<[f64; 3]>], _plane: &Plane) -> f64 {
    // Only the ordering of the candidate list matters; the plane origin is as
    // good a reference as any.
    length([point[0], point[1], point[2]])
}

/// Is a disc of `radius` around `centre` entirely inside the polygon?
fn fits(polygon: &[[f64; 2]], centre: [f64; 2], radius: f64) -> bool {
    if !inside(polygon, centre) {
        return false;
    }
    (0..16).all(|step| {
        let angle = std::f64::consts::TAU * step as f64 / 16.0;
        let test = [
            centre[0] + radius * angle.cos(),
            centre[1] + radius * angle.sin(),
        ];
        inside(polygon, test)
    })
}

/// Ray-crossing test on a polygon in the plane.
fn inside(polygon: &[[f64; 2]], point: [f64; 2]) -> bool {
    let mut inside = false;
    let mut previous = polygon.len() - 1;
    for current in 0..polygon.len() {
        let (a, b) = (polygon[current], polygon[previous]);
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let x = a[0] + (point[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if point[0] < x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

fn plane_u(normal: [f64; 3]) -> [f64; 3] {
    let helper = if normal[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    normalise(cross(normal, helper))
}

fn plane_v(normal: [f64; 3]) -> [f64; 3] {
    normalise(cross(normal, plane_u(normal)))
}

fn add_3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    add(a, scale(sub(b, a), t))
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn as_f64(vertex: [f32; 3]) -> [f64; 3] {
    [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64]
}

fn push_triangle(mesh: &mut TriangleMesh, corners: &[[f64; 3]; 3]) {
    let base = mesh.vertices.len() as u32;
    for corner in corners {
        mesh.vertices
            .push([corner[0] as f32, corner[1] as f32, corner[2] as f32]);
    }
    mesh.triangles.push([base, base + 1, base + 2]);
}

fn fan(mesh: &mut TriangleMesh, polygon: &[[f64; 3]]) {
    if polygon.len() < 3 {
        return;
    }
    let base = mesh.vertices.len() as u32;
    for point in polygon {
        mesh.vertices
            .push([point[0] as f32, point[1] as f32, point[2] as f32]);
    }
    for corner in 1..polygon.len() - 1 {
        mesh.triangles
            .push([base, base + corner as u32, base + corner as u32 + 1]);
    }
}

/// Sutherland–Hodgman against one side of the plane.
fn clip(corners: &[[f64; 3]; 3], distance: &[f64; 3], keep_above: bool) -> Vec<[f64; 3]> {
    let sign = if keep_above { 1.0 } else { -1.0 };
    let inside = |d: f64| d * sign >= -EPS;
    let mut polygon = Vec::with_capacity(4);
    for index in 0..3 {
        let current = corners[index];
        let next = corners[(index + 1) % 3];
        let (d_current, d_next) = (distance[index], distance[(index + 1) % 3]);
        if inside(d_current) {
            polygon.push(current);
        }
        if inside(d_current) != inside(d_next) {
            let t = d_current / (d_current - d_next);
            polygon.push([
                current[0] + (next[0] - current[0]) * t,
                current[1] + (next[1] - current[1]) * t,
                current[2] + (next[2] - current[2]) * t,
            ]);
        }
    }
    polygon
}

/// Where the plane met this triangle, as a pair of points.
fn crossing(corners: [[f64; 3]; 3], distance: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut push = |point: [f64; 3]| {
        if !points.iter().any(|existing| same(existing, &point)) {
            points.push(point);
        }
    };
    for index in 0..3 {
        let next = (index + 1) % 3;
        let (here, there) = (distance[index], distance[next]);
        if here.abs() <= EPS {
            push(corners[index]);
        }
        if (here > EPS && there < -EPS) || (here < -EPS && there > EPS) {
            let t = here / (here - there);
            push([
                corners[index][0] + (corners[next][0] - corners[index][0]) * t,
                corners[index][1] + (corners[next][1] - corners[index][1]) * t,
                corners[index][2] + (corners[next][2] - corners[index][2]) * t,
            ]);
        }
    }
    if points.len() == 2 {
        Some((points[0], points[1]))
    } else {
        // A triangle lying in the plane has no interior, and a vertex sitting
        // exactly on it adds a third point; neither contributes an edge.
        None
    }
}

fn same(a: &[f64; 3], b: &[f64; 3]) -> bool {
    (a[0] - b[0]).abs() <= 1e-6 && (a[1] - b[1]).abs() <= 1e-6 && (a[2] - b[2]).abs() <= 1e-6
}

fn key(point: [f64; 3]) -> (i64, i64, i64) {
    (
        (point[0] * 1e6).round() as i64,
        (point[1] * 1e6).round() as i64,
        (point[2] * 1e6).round() as i64,
    )
}

/// Stitch the cut segments into closed loops, in the plane's own frame.
fn loops(segments: &[([f64; 3], [f64; 3])]) -> Vec<Vec<[f64; 3]>> {
    let mut adjacency: std::collections::HashMap<(i64, i64, i64), Vec<(i64, i64, i64)>> =
        std::collections::HashMap::new();
    let mut points: std::collections::HashMap<(i64, i64, i64), [f64; 3]> =
        std::collections::HashMap::new();
    for (start, end) in segments {
        let (a, b) = (key(*start), key(*end));
        if a == b {
            continue;
        }
        points.insert(a, *start);
        points.insert(b, *end);
        adjacency.entry(a).or_default().push(b);
        adjacency.entry(b).or_default().push(a);
    }
    let mut used: std::collections::HashSet<(i64, i64, i64)> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (start, _) in adjacency.iter() {
        if used.contains(start) {
            continue;
        }
        let mut loop_keys = vec![*start];
        used.insert(*start);
        let mut current = *start;
        loop {
            let Some(neighbours) = adjacency.get(&current) else {
                break;
            };
            // Prefer an edge that has not been walked, so shared corners do not
            // swallow the loop.
            let next = neighbours.iter().find(|n| !used.contains(n)).or_else(|| {
                neighbours
                    .iter()
                    .find(|n| **n == loop_keys[0] && loop_keys.len() > 2)
            });
            let Some(next) = next else { break };
            if *next == loop_keys[0] {
                break;
            }
            used.insert(*next);
            loop_keys.push(*next);
            current = *next;
        }
        if loop_keys.len() >= 3 {
            out.push(loop_keys.iter().map(|k| points[k]).collect());
        }
    }
    out
}

/// Ear-clip a flat loop. The loop is projected onto the plane's own basis, so
/// the arithmetic stays two dimensional.
fn triangulate_cap(polygon: &[[f64; 3]], plane: &Plane) -> Vec<[usize; 3]> {
    let normal = plane.normal;
    // Any pair of vectors perpendicular to the normal will do.
    let helper = if normal[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = cross(helper, normal);
    let u = normalise(u);
    let v = cross(normal, u);
    let flat: Vec<[f64; 2]> = polygon
        .iter()
        .map(|p| [dot(*p, u), dot(*p, v)])
        .collect();
    ear_clip(&flat)
}

fn ear_clip(flat: &[[f64; 2]]) -> Vec<[usize; 3]> {
    let mut indices: Vec<usize> = (0..flat.len()).collect();
    if signed_area(flat) < 0.0 {
        indices.reverse();
    }
    let mut triangles = Vec::new();
    let mut guard = 0;
    while indices.len() > 3 && guard < 10_000 {
        guard += 1;
        let mut clipped = false;
        for position in 0..indices.len() {
            let previous = indices[(position + indices.len() - 1) % indices.len()];
            let current = indices[position];
            let next = indices[(position + 1) % indices.len()];
            if !is_ear(flat, &indices, previous, current, next) {
                continue;
            }
            triangles.push([previous, current, next]);
            indices.remove(position);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if indices.len() == 3 {
        triangles.push([indices[0], indices[1], indices[2]]);
    }
    triangles
}

fn is_ear(flat: &[[f64; 2]], indices: &[usize], previous: usize, current: usize, next: usize) -> bool {
    let (a, b, c) = (flat[previous], flat[current], flat[next]);
    if orient2d(a, b, c) <= 0.0 {
        return false;
    }
    for index in indices {
        if *index == previous || *index == current || *index == next {
            continue;
        }
        if point_in_triangle(flat[*index], a, b, c) {
            return false;
        }
    }
    true
}

fn orient2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn point_in_triangle(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let d1 = orient2d(a, b, p);
    let d2 = orient2d(b, c, p);
    let d3 = orient2d(c, a, p);
    let negative = d1 < -1e-12 || d2 < -1e-12 || d3 < -1e-12;
    let positive = d1 > 1e-12 || d2 > 1e-12 || d3 > 1e-12;
    !(negative && positive)
}

fn signed_area(flat: &[[f64; 2]]) -> f64 {
    let mut total = 0.0;
    for index in 0..flat.len() {
        let a = flat[index];
        let b = flat[(index + 1) % flat.len()];
        total += a[0] * b[1] - b[0] * a[1];
    }
    total * 0.5
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalise(a: [f64; 3]) -> [f64; 3] {
    let length = dot(a, a).sqrt();
    if length > 0.0 {
        [a[0] / length, a[1] / length, a[2] / length]
    } else {
        [1.0, 0.0, 0.0]
    }
}

/// Volume enclosed by a closed mesh, signed by its winding.
pub fn volume(mesh: &TriangleMesh) -> f64 {
    let mut total = 0.0;
    for triangle in &mesh.triangles {
        let a = as_f64(mesh.vertices[triangle[0] as usize]);
        let b = as_f64(mesh.vertices[triangle[1] as usize]);
        let c = as_f64(mesh.vertices[triangle[2] as usize]);
        total += dot(a, cross(b, c)) / 6.0;
    }
    total
}

/// Every edge shared by exactly two triangles means the shell is closed.
pub fn is_closed(mesh: &TriangleMesh) -> bool {
    let mut edges: std::collections::HashMap<((i64, i64, i64), (i64, i64, i64)), i32> =
        std::collections::HashMap::new();
    for triangle in &mesh.triangles {
        let points = [
            key(as_f64(mesh.vertices[triangle[0] as usize])),
            key(as_f64(mesh.vertices[triangle[1] as usize])),
            key(as_f64(mesh.vertices[triangle[2] as usize])),
        ];
        for index in 0..3 {
            let a = points[index];
            let b = points[(index + 1) % 3];
            let edge = if a <= b { (a, b) } else { (b, a) };
            *edges.entry(edge).or_insert(0) += 1;
        }
    }
    !edges.is_empty() && edges.values().all(|count| *count == 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(min: [f32; 3], max: [f32; 3]) -> TriangleMesh {
        let mut mesh = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        for corner in 0..8 {
            mesh.vertices.push([
                if corner & 1 == 0 { min[0] } else { max[0] },
                if corner & 2 == 0 { min[1] } else { max[1] },
                if corner & 4 == 0 { min[2] } else { max[2] },
            ]);
        }
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        for quad in quads {
            mesh.triangles.push([quad[0], quad[1], quad[2]]);
            mesh.triangles.push([quad[0], quad[2], quad[3]]);
        }
        mesh
    }

    #[test]
    fn a_cube_cut_in_half_gives_two_closed_halves() {
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let halves = cut(&cube, &Plane::new([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]));
        assert!(is_closed(&halves.above), "the top half must be watertight");
        assert!(is_closed(&halves.below), "the bottom half must be watertight");
        let (top, bottom) = (volume(&halves.above), volume(&halves.below));
        assert!((top - 500.0).abs() < 1e-3, "top half is 5 x 10 x 10: {top}");
        assert!((bottom - 500.0).abs() < 1e-3, "bottom half is 5 x 10 x 10: {bottom}");
        assert!((top + bottom - volume(&cube)).abs() < 1e-3, "no plastic lost or gained");
    }

    #[test]
    fn cutting_twice_adds_up_to_the_original() {
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let original = volume(&cube);
        let once = cut(&cube, &Plane::new([0.0, 0.0, 4.0], [0.0, 0.0, 1.0]));
        let again = cut(&once.above, &Plane::new([0.0, 0.0, 7.5], [0.0, 0.0, 1.0]));
        let total = volume(&again.below) + volume(&again.above) + volume(&once.below);
        assert!((total - original).abs() < 1e-2, "{total} against {original}");
        assert!(is_closed(&again.above) && is_closed(&again.below));
    }

    #[test]
    fn a_cut_that_misses_leaves_one_side_empty() {
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let halves = cut(&cube, &Plane::new([0.0, 0.0, 20.0], [0.0, 0.0, 1.0]));
        assert!(halves.is_empty());
        assert!((volume(&halves.below) - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn a_pinned_cut_leaves_a_pin_on_one_half_and_a_hole_in_the_other() {
        let cube = boxed([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        let plane = Plane::new([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]);
        let halves = cut_with_pins(
            &cube,
            &plane,
            Pin {
                radius: 2.0,
                height: 4.0,
                clearance: 0.2,
            },
            2,
        );
        let lower = volume(&halves.below);
        let upper = volume(&halves.above);
        // A pin adds material below the plane; its hole takes material away
        // above it. Neither half is the plain half any more.
        assert!(lower > 4000.0 + 20.0, "the lower half should have pins: {lower}");
        assert!(upper < 4000.0 - 20.0, "the upper half should have holes: {upper}");
        // Two pins, so roughly twice the hole.
        let one_hole = std::f64::consts::PI * 2.2f64.powi(2) * 4.4;
        assert!(
            (4000.0 - upper - 2.0 * one_hole).abs() < one_hole * 0.3,
            "holes should account for {:.1} mm3, missing {:.1}",
            2.0 * one_hole,
            4000.0 - upper
        );
    }

    #[test]
    fn a_pin_never_lands_outside_a_narrow_face() {
        // Ten millimetres wide, and the pin needs 2 + 0.2 + 0.6 of clearance on
        // every side: one fits in the middle and no more.
        let bar = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let plane = Plane::new([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]);
        let halves = cut_with_pins(&bar, &plane, Pin::default(), 6);
        let added = volume(&halves.below) - 500.0;
        let one_pin = std::f64::consts::PI * 4.0 * 5.0;
        let pins = (added / one_pin).round();
        assert!(pins <= 1.0, "{pins} pins were placed on a 10 mm face");
    }

    #[test]
    fn a_diagonal_cut_still_closes_and_conserves() {
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let halves = cut(&cube, &Plane::new([5.0, 5.0, 5.0], [1.0, 1.0, 1.0]));
        assert!(is_closed(&halves.above) && is_closed(&halves.below));
        let total = volume(&halves.above) + volume(&halves.below);
        assert!((total - 1000.0).abs() < 1e-2, "{total}");
        // Symmetric cut through the middle splits the cube in two equal parts.
        assert!((volume(&halves.above) - 500.0).abs() < 1.0, "{}", volume(&halves.above));
    }
    #[test]
    fn a_dovetail_stands_proud_of_the_face_and_leaves_a_slot_for_itself() {
        let cube = boxed([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        let plane = Plane::new([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]);
        let dovetail = Dovetail {
            root: 4.0,
            tip: 8.0,
            depth: 4.0,
            thickness: 8.0,
            clearance: 0.2,
        };
        let joint = cut_with_dovetail(&cube, &plane, dovetail);
        let halves = &joint.halves;
        let plain = cut(&cube, &plane);
        let lower = volume(&halves.below);
        let upper = volume(&halves.above);
        // The tab is a trapezoid 4 mm at the face, 8 mm at the tip, 4 mm deep
        // and 8 mm thick: 24 mm2 of profile, 192 mm3 of plastic, added below
        // the plane.
        assert!(
            (lower - (volume(&plain.below) + 192.0)).abs() < 2.0,
            "the lower half should carry the tab: {lower}"
        );
        // The slot takes that 192 mm3 away, plus the clearance around it.
        assert!(
            upper < volume(&plain.above) - 190.0 && upper > volume(&plain.above) - 260.0,
            "the upper half should hold the slot: {upper}"
        );
        // BSP leaves seams where it clips a polygon against its neighbour, so
        // "closed" here means a few open edges, not none.
        let seams = csg::open_edges(&halves.above);
        assert!(seams <= 40, "the slotted half should be all but closed: {seams} open edges");
        let lower_seams = csg::open_edges(&halves.below);
        assert!(lower_seams <= 40, "the tabbed half should be all but closed: {lower_seams}");
        // It really is a dovetail: the tab reaches past the plane it was cut on.
        let (_, top) = halves.below.bounds();
        assert!((top[2] as f64 - 14.0).abs() < 1e-3, "the tab is 4 mm proud: {top:?}");
        let reported = joint.tab_mm3.expect("a tab was cut, so it has a volume");
        assert!((reported - 192.0).abs() < 1.0, "the tab reports 192 mm3: {reported}");
    }

    #[test]
    fn a_flat_plate_takes_a_dovetail_the_size_it_was_asked_for() {
        // The bracket's real dimensions: 70 mm square, 11 mm tall, cut through
        // the middle. A full-size tab would be 8 mm deep, but the upper half is
        // only 5.5 mm thick, so it is held a millimetre short of coming out of
        // the top: 4.5 mm deep, 8 mm at the root, 16 mm at the tip, 8 mm thick.
        // That is 54 mm2 of profile and 432 mm3 of plastic.
        let plate = boxed([0.0, 0.0, 0.0], [70.0, 70.0, 11.0]);
        let plane = Plane::new([0.0, 0.0, 5.5], [0.0, 0.0, 1.0]);
        let plain = cut(&plate, &plane);
        let joint = cut_with_dovetail(&plate, &plane, Dovetail::from_root(8.0));
        let halves = &joint.halves;
        let gained = volume(&halves.below) - volume(&plain.below);
        assert!(
            (joint.tab_mm3.expect("the plate takes a tab") - 432.0).abs() < 2.0,
            "the plate reports the tab it took: {:?}",
            joint.tab_mm3
        );
        assert!((gained - 432.0).abs() < 2.0, "the tab should add 432 mm3: {gained}");
        let (_, top) = halves.below.bounds();
        assert!((top[2] as f64 - 10.0).abs() < 1e-3, "the tab stops 1 mm under the surface: {top:?}");
        assert!(csg::open_edges(&halves.above) <= 40);
        assert!(csg::open_edges(&halves.below) <= 40);
    }

    #[test]
    fn a_part_too_narrow_for_a_dovetail_is_cut_plain() {
        let thin = boxed([0.0, 0.0, 0.0], [6.0, 6.0, 20.0]);
        let plane = Plane::new([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]);
        let plain = cut(&thin, &plane);
        let joint = cut_with_dovetail(
            &thin,
            &plane,
            Dovetail {
                root: 8.0,
                tip: 16.0,
                depth: 6.0,
                thickness: 14.0,
                clearance: 0.2,
            },
        );
        let halves = &joint.halves;
        assert!(
            (volume(&halves.below) - volume(&plain.below)).abs() < 1e-2,
            "a tab that does not fit must not be forced in: {}",
            volume(&halves.below)
        );
        // And the caller can say so: a refinement that was asked for and could
        // not be honoured is not the same answer as one that was.
        assert!(
            joint.tab_mm3.is_none(),
            "a plain cut reports no tab, not a zero one: {:?}",
            joint.tab_mm3
        );
        assert!(is_closed(&halves.below) && is_closed(&halves.above));
    }
}
