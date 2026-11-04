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
    fn a_diagonal_cut_still_closes_and_conserves() {
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let halves = cut(&cube, &Plane::new([5.0, 5.0, 5.0], [1.0, 1.0, 1.0]));
        assert!(is_closed(&halves.above) && is_closed(&halves.below));
        let total = volume(&halves.above) + volume(&halves.below);
        assert!((total - 1000.0).abs() < 1e-2, "{total}");
        // Symmetric cut through the middle splits the cube in two equal parts.
        assert!((volume(&halves.above) - 500.0).abs() < 1.0, "{}", volume(&halves.above));
    }
}
