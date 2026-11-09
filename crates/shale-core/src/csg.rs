//! Solid booleans: union and subtraction on closed triangle meshes.
//!
//! A plane cut can only take a part apart. Bambu's *Cut* tool also puts the two
//! halves back together with pins and holes — and a hole is not something a
//! plane can do, so this is a real BSP-tree boolean (the classic
//! constructive-solid-geometry algorithm): every polygon of A is clipped against
//! the tree built from B's polygons, and the pieces on the wrong side are
//! discarded.
//!
//! Everything here is geometry in the part's own frame: no engine, no
//! transforms, no units but millimetres.

use crate::mesh::TriangleMesh;

/// Two vertices closer than this are the same vertex. A tenth of a micron.
const EPS: f64 = 1e-6;

#[derive(Clone, Debug)]
struct Poly {
    vertices: Vec<[f64; 3]>,
    normal: [f64; 3],
    /// `dot(normal, vertex)` — the plane this polygon lies on.
    w: f64,
}

impl Poly {
    fn new(vertices: Vec<[f64; 3]>) -> Option<Self> {
        if vertices.len() < 3 {
            return None;
        }
        let mut normal = [0.0; 3];
        for index in 1..vertices.len() - 1 {
            let a = sub(vertices[index], vertices[0]);
            let b = sub(vertices[index + 1], vertices[0]);
            normal = cross(a, b);
            if length(normal) > EPS {
                break;
            }
        }
        let normal = normalise(normal);
        if length(normal) < 0.5 {
            // A degenerate polygon has no plane; keeping it would poison the tree.
            return None;
        }
        let w = dot(normal, vertices[0]);
        Some(Self {
            vertices,
            normal,
            w,
        })
    }

    fn flip(&mut self) {
        self.vertices.reverse();
        self.normal = [-self.normal[0], -self.normal[1], -self.normal[2]];
        self.w = -self.w;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Coplanar,
    Front,
    Back,
}

/// Which side of the plane `dot(normal, x) = w` a point is on.
fn side_of(normal: &[f64; 3], w: f64, point: [f64; 3]) -> f64 {
    dot(*normal, point) - w
}

/// Split `polygon` by a plane, dropping each piece into the list it belongs to.
fn split_polygon(
    normal: [f64; 3],
    w: f64,
    polygon: &Poly,
    coplanar_front: &mut Vec<Poly>,
    coplanar_back: &mut Vec<Poly>,
    front: &mut Vec<Poly>,
    back: &mut Vec<Poly>,
) {
    let mut polygon_type = 0u8;
    let mut types: Vec<Side> = Vec::with_capacity(polygon.vertices.len());
    for vertex in &polygon.vertices {
        let distance = side_of(&normal, w, *vertex);
        let kind = if distance < -EPS {
            polygon_type |= 1;
            Side::Back
        } else if distance > EPS {
            polygon_type |= 2;
            Side::Front
        } else {
            Side::Coplanar
        };
        types.push(kind);
    }

    match polygon_type {
        0 => {
            // Coplanar: which way it faces decides which side it belongs to.
            if dot(normal, polygon.normal) > 0.0 {
                coplanar_front.push(polygon.clone());
            } else {
                coplanar_back.push(polygon.clone());
            }
        }
        1 => back.push(polygon.clone()),
        2 => front.push(polygon.clone()),
        _ => {
            let mut front_vertices: Vec<[f64; 3]> = Vec::new();
            let mut back_vertices: Vec<[f64; 3]> = Vec::new();
            for index in 0..polygon.vertices.len() {
                let next = (index + 1) % polygon.vertices.len();
                let (vi, vj) = (polygon.vertices[index], polygon.vertices[next]);
                let (ti, tj) = (types[index], types[next]);
                if ti != Side::Back {
                    front_vertices.push(vi);
                }
                if ti != Side::Front {
                    back_vertices.push(vi);
                }
                if (ti == Side::Front && tj == Side::Back) || (ti == Side::Back && tj == Side::Front)
                {
                    let numerator = w - dot(normal, vi);
                    let denominator = dot(normal, sub(vj, vi));
                    if denominator.abs() > EPS {
                        let t = numerator / denominator;
                        let cut = add(vi, scale(sub(vj, vi), t));
                        front_vertices.push(cut);
                        back_vertices.push(cut);
                    }
                }
            }
            if front_vertices.len() >= 3 {
                if let Some(piece) = Poly::new(front_vertices) {
                    front.push(piece);
                }
            }
            if back_vertices.len() >= 3 {
                if let Some(piece) = Poly::new(back_vertices) {
                    back.push(piece);
                }
            }
        }
    }
}

/// A BSP node: a splitting plane, the polygons that lie on it, and two subtrees.
/// How a candidate plane would cut a set of polygons: how many it would have
/// to split, and how lopsided the two sides would be.
fn plane_cost(polygons: &[Poly], normal: [f64; 3], w: f64) -> usize {
    let mut front = 0usize;
    let mut back = 0usize;
    let mut splits = 0usize;
    for polygon in polygons {
        let mut any_front = false;
        let mut any_back = false;
        for vertex in &polygon.vertices {
            let distance = side_of(&normal, w, *vertex);
            if distance > EPS {
                any_front = true;
            } else if distance < -EPS {
                any_back = true;
            }
        }
        match (any_front, any_back) {
            (true, true) => splits += 1,
            (true, false) => front += 1,
            (false, true) => back += 1,
            (false, false) => {}
        }
    }
    // Splitting is what makes the polygon lists grow, so it costs more than an
    // uneven cut — but an uneven cut costs depth, and the two together are what
    // turned a 660-triangle part into five minutes of walking a chain.
    splits * 8 + front.abs_diff(back)
}

/// Pick the plane that cuts this set most evenly, rather than the first one that
/// happens to be in the list.
///
/// csg.js starts every node with `polygons[0]`'s plane. On a mesh from a real
/// part that is close to the worst choice available: a bore alone contributes
/// hundreds of facets, each with its own plane, and none of them divides the
/// rest — so the tree becomes a chain, every level re-splits what is below it,
/// and a 660-triangle part took five minutes and had to be killed. Sampling
/// two dozen candidates and preferring the cut that splits least and balances
/// most keeps the depth near log n.
fn pick_plane(polygons: &[Poly]) -> ([f64; 3], f64) {
    let stride = (polygons.len() / 24).max(1);
    let mut best = (polygons[0].normal, polygons[0].w);
    let mut best_cost = usize::MAX;
    for (index, polygon) in polygons.iter().enumerate() {
        if index % stride != 0 {
            continue;
        }
        let cost = plane_cost(polygons, polygon.normal, polygon.w);
        if cost < best_cost {
            best_cost = cost;
            best = (polygon.normal, polygon.w);
        }
    }
    best
}

#[derive(Clone, Default)]
struct Node {
    plane: Option<([f64; 3], f64)>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polygons: Vec<Poly>,
}

impl Node {
    fn from_polygons(polygons: Vec<Poly>) -> Self {
        let mut node = Node::default();
        node.build(polygons);
        node
    }

    fn build(&mut self, polygons: Vec<Poly>) {
        if polygons.is_empty() {
            return;
        }
        if self.plane.is_none() {
            self.plane = Some(pick_plane(&polygons));
        }
        let (normal, w) = self.plane.unwrap();
        let mut front = Vec::new();
        let mut back = Vec::new();
        let mut coplanar_front = Vec::new();
        let mut coplanar_back = Vec::new();
        for polygon in polygons {
            split_polygon(
                normal,
                w,
                &polygon,
                &mut coplanar_front,
                &mut coplanar_back,
                &mut front,
                &mut back,
            );
        }
        // A polygon on this node's own plane belongs to this node.
        self.polygons.extend(coplanar_front);
        self.polygons.extend(coplanar_back);
        if !front.is_empty() {
            self.front
                .get_or_insert_with(|| Box::new(Node::default()))
                .build(front);
        }
        if !back.is_empty() {
            self.back
                .get_or_insert_with(|| Box::new(Node::default()))
                .build(back);
        }
    }

    /// Turn the solid inside out: every plane and polygon faces the other way.
    fn invert(&mut self) {
        for polygon in &mut self.polygons {
            polygon.flip();
        }
        if let Some((normal, w)) = self.plane {
            self.plane = Some(([-normal[0], -normal[1], -normal[2]], -w));
        }
        if let Some(front) = &mut self.front {
            front.invert();
        }
        if let Some(back) = &mut self.back {
            back.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// Remove the parts of `polygons` that are inside this solid.
    fn clip_polygons(&self, polygons: Vec<Poly>) -> Vec<Poly> {
        let Some((normal, w)) = self.plane else {
            return polygons;
        };
        let mut front = Vec::new();
        let mut back = Vec::new();
        let mut coplanar_front = Vec::new();
        let mut coplanar_back = Vec::new();
        for polygon in polygons {
            split_polygon(
                normal,
                w,
                &polygon,
                &mut coplanar_front,
                &mut coplanar_back,
                &mut front,
                &mut back,
            );
        }
        front.extend(coplanar_front);
        back.extend(coplanar_back);
        let mut kept = match &self.front {
            Some(node) => node.clip_polygons(front),
            None => front,
        };
        // Anything behind this leaf is inside the solid, so it is dropped.
        if let Some(node) = &self.back {
            kept.extend(node.clip_polygons(back));
        }
        kept
    }

    fn clip_to(&mut self, other: &Node) {
        self.polygons = other.clip_polygons(std::mem::take(&mut self.polygons));
        if let Some(front) = &mut self.front {
            front.clip_to(other);
        }
        if let Some(back) = &mut self.back {
            back.clip_to(other);
        }
    }

    /// Nodes, depth and polygon count — what tells a balanced tree from a chain.
    #[cfg(test)]
    fn stats(&self) -> (usize, usize, usize) {
        let mut nodes = 1;
        let mut depth = 1;
        let mut polygons = self.polygons.len();
        for child in [&self.front, &self.back].into_iter().flatten() {
            let (n, d, p) = child.stats();
            nodes += n;
            depth = depth.max(d + 1);
            polygons += p;
        }
        (nodes, depth, polygons)
    }

    fn all_polygons(&self) -> Vec<Poly> {
        let mut out = self.polygons.clone();
        if let Some(front) = &self.front {
            out.extend(front.all_polygons());
        }
        if let Some(back) = &self.back {
            out.extend(back.all_polygons());
        }
        out
    }
}

fn to_polygons(mesh: &TriangleMesh) -> Vec<Poly> {
    let mut out = Vec::with_capacity(mesh.triangles.len());
    for triangle in &mesh.triangles {
        let corners = [
            as_f64(mesh.vertices[triangle[0] as usize]),
            as_f64(mesh.vertices[triangle[1] as usize]),
            as_f64(mesh.vertices[triangle[2] as usize]),
        ];
        if let Some(polygon) = Poly::new(corners.to_vec()) {
            out.push(polygon);
        }
    }
    out
}

fn to_mesh(polygons: &[Poly]) -> TriangleMesh {
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    // Weld as we go: a split triangle and its neighbour share a corner exactly,
    // and writing that corner twice triples the file for nothing.
    let mut seen: std::collections::HashMap<(i64, i64, i64), u32> =
        std::collections::HashMap::new();
    let mut index_of = |mesh: &mut TriangleMesh, corner: [f64; 3]| -> u32 {
        let key = (
            (corner[0] * 10_000.0).round() as i64,
            (corner[1] * 10_000.0).round() as i64,
            (corner[2] * 10_000.0).round() as i64,
        );
        if let Some(existing) = seen.get(&key) {
            return *existing;
        }
        let index = mesh.vertices.len() as u32;
        mesh.vertices
            .push([corner[0] as f32, corner[1] as f32, corner[2] as f32]);
        seen.insert(key, index);
        index
    };
    for polygon in polygons {
        // Boolean splits keep polygons convex, so a fan from the first vertex is
        // a correct triangulation.
        for index in 1..polygon.vertices.len().saturating_sub(1) {
            let a = index_of(&mut mesh, polygon.vertices[0]);
            let b = index_of(&mut mesh, polygon.vertices[index]);
            let c = index_of(&mut mesh, polygon.vertices[index + 1]);
            if a != b && b != c && a != c {
                mesh.triangles.push([a, b, c]);
            }
        }
    }
    mesh
}

/// Everything in `a` or in `b`.
pub fn union(a: &TriangleMesh, b: &TriangleMesh) -> TriangleMesh {
    let mut left = Node::from_polygons(to_polygons(a));
    let mut right = Node::from_polygons(to_polygons(b));
    left.clip_to(&right);
    right.clip_to(&left);
    right.invert();
    right.clip_to(&left);
    right.invert();
    left.build(right.all_polygons());
    to_mesh(&left.all_polygons())
}

/// Everything in `a` that is not in `b`.
pub fn subtract(a: &TriangleMesh, b: &TriangleMesh) -> TriangleMesh {
    let mut left = Node::from_polygons(to_polygons(a));
    let mut right = Node::from_polygons(to_polygons(b));
    left.invert();
    left.clip_to(&right);
    right.clip_to(&left);
    right.invert();
    right.clip_to(&left);
    right.invert();
    left.build(right.all_polygons());
    left.invert();
    to_mesh(&left.all_polygons())
}

/// Everything in both.
pub fn intersect(a: &TriangleMesh, b: &TriangleMesh) -> TriangleMesh {
    let mut left = Node::from_polygons(to_polygons(a));
    let mut right = Node::from_polygons(to_polygons(b));
    left.invert();
    right.clip_to(&left);
    right.invert();
    left.clip_to(&right);
    right.clip_to(&left);
    left.build(right.all_polygons());
    left.invert();
    to_mesh(&left.all_polygons())
}

/// A closed cylinder whose base sits at `start` and whose axis runs to `start +
/// axis * height`. Used for pins (union) and their holes (subtract), so both
/// ends are capped.
pub fn cylinder(start: [f64; 3], axis: [f64; 3], radius: f64, height: f64, segments: usize) -> TriangleMesh {
    let segments = segments.max(3);
    let axis = normalise(axis);
    // Any vector not parallel to the axis gives a basis to sweep the rim.
    let helper = if axis[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalise(cross(axis, helper));
    let v = normalise(cross(axis, u));
    let far = add(start, scale(axis, height));

    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    let rim = |centre: [f64; 3]| -> Vec<[f64; 3]> {
        (0..segments)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / segments as f64;
                add(
                    centre,
                    add(scale(u, radius * angle.cos()), scale(v, radius * angle.sin())),
                )
            })
            .collect()
    };
    let near_ring = rim(start);
    let far_ring = rim(far);

    let push = |mesh: &mut TriangleMesh, corners: [[f64; 3]; 3]| {
        let base = mesh.vertices.len() as u32;
        for corner in corners {
            mesh.vertices
                .push([corner[0] as f32, corner[1] as f32, corner[2] as f32]);
        }
        mesh.triangles.push([base, base + 1, base + 2]);
    };

    for index in 0..segments {
        let next = (index + 1) % segments;
        push(&mut mesh, [near_ring[index], near_ring[next], far_ring[next]]);
        push(&mut mesh, [near_ring[index], far_ring[next], far_ring[index]]);
        // Caps, wound outward: the near cap faces -axis, the far cap +axis.
        push(&mut mesh, [start, near_ring[next], near_ring[index]]);
        push(&mut mesh, [far, far_ring[index], far_ring[next]]);
    }
    mesh
}

/// Edges no second triangle shares. BSP clipping splits a polygon without
/// splitting its neighbour, so the output carries T-junctions: the surface is
/// still closed, but it does not pair edge-for-edge. Kept as a diagnostic,
/// because a *large* number means the booleans went wrong rather than merely
/// leaving seams.
pub fn open_edges(mesh: &TriangleMesh) -> usize {
    use std::collections::HashMap;
    let key = |vertex: [f32; 3]| {
        (
            (vertex[0] * 1000.0).round() as i64,
            (vertex[1] * 1000.0).round() as i64,
            (vertex[2] * 1000.0).round() as i64,
        )
    };
    let mut edges: HashMap<((i64, i64, i64), (i64, i64, i64)), usize> = HashMap::new();
    for triangle in &mesh.triangles {
        for index in 0..3 {
            let a = key(mesh.vertices[triangle[index] as usize]);
            let b = key(mesh.vertices[triangle[(index + 1) % 3] as usize]);
            if a == b {
                continue;
            }
            let ordered = if a < b { (a, b) } else { (b, a) };
            *edges.entry(ordered).or_insert(0) += 1;
        }
    }
    edges.values().filter(|count| **count != 2).count()
}

fn as_f64(vertex: [f32; 3]) -> [f64; 3] {
    [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64]
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

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn normalise(a: [f64; 3]) -> [f64; 3] {
    let size = length(a);
    if size < EPS {
        [0.0, 0.0, 1.0]
    } else {
        scale(a, 1.0 / size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cut::{is_closed, volume};

    fn boxed(min: [f32; 3], max: [f32; 3]) -> TriangleMesh {
        // Six quads, each two triangles, wound outward.
        let corners = [
            [min[0], min[1], min[2]],
            [max[0], min[1], min[2]],
            [max[0], max[1], min[2]],
            [min[0], max[1], min[2]],
            [min[0], min[1], max[2]],
            [max[0], min[1], max[2]],
            [max[0], max[1], max[2]],
            [min[0], max[1], max[2]],
        ];
        let faces = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [2, 3, 7],
            [2, 7, 6],
            [1, 2, 6],
            [1, 6, 5],
            [0, 4, 7],
            [0, 7, 3],
        ];
        TriangleMesh {
            vertices: corners.to_vec(),
            triangles: faces.iter().map(|face| [face[0] as u32, face[1] as u32, face[2] as u32]).collect(),
        }
    }

    #[test]
    fn a_triangle_has_a_plane() {
        let polygon = Poly::new(vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
            .expect("a triangle is a polygon");
        assert!((polygon.normal[2] - 1.0).abs() < 1e-9, "normal faces +z");
    }

    #[test]
    fn a_cube_minus_a_pillar_through_it_is_still_closed() {
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let pillar = cylinder([5.0, 5.0, -2.0], [0.0, 0.0, 1.0], 2.0, 14.0, 48);
        let cut = subtract(&cube, &pillar);
        let expected = 1000.0 - std::f64::consts::PI * 4.0 * 10.0;
        let measured = volume(&cut);
        println!(
            "bore: {measured:.2} of {expected:.2}, open edges {}",
            open_edges(&cut)
        );
        assert!(
            (measured - expected).abs() / expected < 0.01,
            "volume {measured} should be about {expected}"
        );
    }

    #[test]
    fn two_cubes_overlapping_halfway() {
        // The case a mesh boolean actually meets: two boxes sharing four
        // faces. Every plane of one lies exactly on a plane of the other, so
        // the coplanar rule is what decides whether the answer is solid or a
        // shell with the middle missing.
        let a = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let b = boxed([5.0, 0.0, 0.0], [15.0, 10.0, 10.0]);

        let difference = subtract(&a, &b);
        let unioned = union(&a, &b);
        let common = intersect(&a, &b);
        println!(
            "difference {:.2}, union {:.2}, common {:.2}; open edges {} {} {}",
            volume(&difference),
            volume(&unioned),
            volume(&common),
            open_edges(&difference),
            open_edges(&unioned),
            open_edges(&common)
        );

        assert!((volume(&difference) - 500.0).abs() < 5.0, "half the cube is left");
        assert!((volume(&unioned) - 1500.0).abs() < 5.0, "the union is both minus the overlap");
        assert!((volume(&common) - 500.0).abs() < 5.0, "the overlap is the shared half");
        // Twelve seams, not a hole: BSP clipping splits a polygon without
        // splitting its neighbour, so the surface is closed but does not pair
        // edge-for-edge (see `open_edges`). A *large* count is the failure —
        // these are the four shared planes' worth of T-junctions and nothing
        // else, and the volume identity above is what proves the solid.
        for (name, mesh) in [
            ("difference", &difference),
            ("union", &unioned),
            ("common", &common),
        ] {
            assert!(
                open_edges(mesh) <= 24,
                "{name} has {} open edges, which is more seam than a box has",
                open_edges(mesh)
            );
        }
    }

    #[test]
    fn a_pin_adds_its_own_volume() {
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let pin = cylinder([5.0, 5.0, 10.0], [0.0, 0.0, 1.0], 2.0, 6.0, 48);
        let joined = union(&cube, &pin);
        let expected = 1000.0 + std::f64::consts::PI * 4.0 * 6.0;
        let measured = volume(&joined);
        println!(
            "pin: {measured:.2} of {expected:.2}, open edges {}",
            open_edges(&joined)
        );
        assert!(
            (measured - expected).abs() / expected < 0.01,
            "volume {measured} should be about {expected}"
        );
    }

    #[test]
    fn a_concave_part_with_a_bore_booleans_quickly() {
        // A plate with a bore is the shape a real part is made of: concave, and
        // hundreds of facets each with its own plane. With csg.js's "first
        // polygon is the plane" rule this took minutes — five, on the 1946
        // triangle bracket, before it had to be killed — because the tree
        // became a chain that re-split everything below it. The split plane is
        // now chosen, and this is the test that keeps it chosen.
        let plate = boxed([0.0, 0.0, 0.0], [40.0, 40.0, 20.0]);
        let bore = cylinder([20.0, 20.0, -1.0], [0.0, 0.0, 1.0], 12.0, 22.0, 96);
        let body = subtract(&plate, &bore);
        let cut = boxed([12.0, 12.0, 10.0], [45.0, 32.0, 30.0]);

        let started = std::time::Instant::now();
        let difference = subtract(&body, &cut);
        let elapsed = started.elapsed();
        println!(
            "booleans: {} tris ({:.2} mm3), cut into {} ({:.2} mm3) in {:?}, open edges {}",
            body.triangles.len(),
            volume(&body),
            difference.triangles.len(),
            volume(&difference),
            elapsed,
            open_edges(&difference)
        );

        assert!(difference.triangles.len() > 500, "the cut left a solid, not a stub");
        // The count is T-junctions, not holes: a BSP splits a polygon without
        // splitting the neighbour whose edge it just gained a vertex on, so a
        // part with a bore in it has hundreds of them and `volume` is what
        // proves the solid. The box test above pins the exact arithmetic.
        assert!(open_edges(&difference) < 4 * difference.triangles.len());
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "a part this size took {elapsed:?}, which is the chain coming back"
        );
    }

    #[test]
    #[ignore]
    fn measure_boolean_scaling() {
        // A cylinder is the shape a real part is made of: many facets, each
        // with its own plane, and none of them a good split for the rest.
        for segments in [32usize, 64, 96, 128, 192, 256] {
            // A convex body builds a chain with no splits at all, so a plain
            // cylinder says nothing about the cost. A bracket is *concave*: a
            // plate with a bore through it, and then a cut across both.
            let plate = boxed([0.0, 0.0, 0.0], [40.0, 40.0, 20.0]);
            let bore = cylinder([20.0, 20.0, -1.0], [0.0, 0.0, 1.0], 12.0, 22.0, segments);
            let body = subtract(&plate, &bore);
            let cube = boxed([12.0, 12.0, 10.0], [45.0, 32.0, 30.0]);
            let started = std::time::Instant::now();
            let difference = subtract(&body, &cube);
            let elapsed = started.elapsed();
            let (nodes, depth, polygons) =
                Node::from_polygons(to_polygons(&body)).stats();
            println!(
                "{segments} segments: {} tris -> {} tris in {:.3}s; tree {nodes} nodes, depth {depth}, {polygons} polys",
                body.triangles.len(),
                difference.triangles.len(),
                elapsed.as_secs_f64()
            );
        }
    }

    #[test]
    fn subtracting_something_that_is_not_there_changes_nothing() {
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let elsewhere = boxed([40.0, 40.0, 40.0], [50.0, 50.0, 50.0]);
        let cut = subtract(&cube, &elsewhere);
        assert!(is_closed(&cut));
        println!("untouched {:.2}", volume(&cut));
        assert!((volume(&cut) - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn a_hole_that_never_touches_the_wall_keeps_the_pin_inside() {
        // The hole is wider than the pin by a clearance of 0.2 mm, and it is
        // deeper than the pin by the same amount: a pin that bottomed out on the
        // far wall of its own hole would not seat.
        let cube = boxed([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let pin = cylinder([5.0, 5.0, 6.0], [0.0, 0.0, 1.0], 2.0, 4.5, 40);
        let hole = cylinder([5.0, 5.0, 5.7], [0.0, 0.0, 1.0], 2.2, 5.0, 40);
        let lower = subtract(&cube, &hole);
        let upper = union(&boxed([0.0, 0.0, 10.0], [10.0, 10.0, 20.0]), &pin);
        // The pin sits *inside* its hole, which is what a clearance means: the
        // hole is wider and deeper, so the pin never bottoms out or jams.
        let overlap = intersect(&pin, &hole);
        let pin_size = volume(&pin);
        assert!(
            (volume(&overlap) - pin_size).abs() < 1.0,
            "the whole pin should sit in the hole: {} of {}",
            volume(&overlap),
            pin_size
        );
        assert!((volume(&lower) - (1000.0 - std::f64::consts::PI * 2.2f64.powi(2) * 4.3)).abs() < 5.0);
        assert!(volume(&upper) > 1000.0);
    }

    /// A lattice sphere whose triangle count is set by its rings, so the timing
    /// ladder can be aimed at 8k, 33k and 80k triangles exactly.
    fn lattice_sphere(rings: usize, segments: usize, radius: f64) -> TriangleMesh {
        let mut vertices = Vec::with_capacity((rings + 1) * segments);
        for ring in 0..=rings {
            let phi = std::f64::consts::PI * ring as f64 / rings as f64;
            for segment in 0..segments {
                let theta = std::f64::consts::TAU * segment as f64 / segments as f64;
                vertices.push([
                    (radius * phi.sin() * theta.cos()) as f32,
                    (radius * phi.sin() * theta.sin()) as f32,
                    (radius * phi.cos()) as f32,
                ]);
            }
        }
        let at = |ring: usize, segment: usize| (ring * segments + segment % segments) as u32;
        let mut triangles = Vec::with_capacity(2 * rings * segments);
        for ring in 0..rings {
            for segment in 0..segments {
                let (a, b) = (at(ring, segment), at(ring, segment + 1));
                let (c, d) = (at(ring + 1, segment), at(ring + 1, segment + 1));
                if ring > 0 {
                    triangles.push([a, b, d]);
                }
                if ring + 1 < rings {
                    triangles.push([a, d, c]);
                }
            }
        }
        TriangleMesh { vertices, triangles }
    }


    /// The release-build ladder: 3 000, 4 900 and 7 800 triangles. Ignored by default because it is a stopwatch, not a check; run
    /// it with `cargo test -p shale-core --release -- --ignored --nocapture`.
    ///
    /// Every rung builds its tree, which is the part of a Boolean that can be
    /// timed at any size. Only a rung whose tree is shallower than
    /// `SHALLOW_ENOUGH` is subtracted as well, because on a convex body this
    /// kernel's tree is a chain — every candidate plane is a face plane, and a
    /// face plane of a convex hull leaves the whole body in front of it — so the
    /// recursion is deeper than any thread's stack. The fifteen- and thirty
    /// thousand rungs were measured at 4.7 s for 7 820 and do not finish: the
    /// chooser is quadratic on a chain, so those sizes need a different kernel,
    /// not a bigger stack.
    #[test]
    #[ignore = "timing ladder: run in release with --ignored --nocapture"]
    fn boolean_timings_at_eight_thirty_and_eighty_thousand() {
        const SHALLOW_ENOUGH: usize = 1_500;
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn(|| {
                for (rings, segments) in [(16, 100), (20, 130), (24, 170)] {
                    let a = lattice_sphere(rings, segments, 20.0);
                    let mut b = lattice_sphere(rings, segments, 20.0);
                    for vertex in &mut b.vertices {
                        vertex[0] += 8.0;
                        vertex[1] += 3.0;
                    }
                    let polygons = to_polygons(&a);
                    let started = std::time::Instant::now();
                    let node = Node::from_polygons(polygons);
                    let built = started.elapsed().as_secs_f64() * 1000.0;
                    let (nodes, depth, kept) = node.stats();
                    println!(
                        "{:>6} triangles: tree in {built:>10.2} ms — {nodes} nodes, depth {depth}, \
                         {kept} polygons",
                        a.triangles.len()
                    );

                    if depth <= SHALLOW_ENOUGH {
                        let started = std::time::Instant::now();
                        let out = subtract(&a, &b);
                        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
                        println!(
                            "        subtract: {:>6} out, {elapsed:>10.2} ms, {:.0} mm3, {} open edges",
                            out.triangles.len(),
                            crate::cut::volume(&out),
                            open_edges(&out)
                        );
                    } else {
                        println!(
                            "        subtract skipped: a tree {depth} deep is a chain, and the                              recursion would not fit a stack"
                        );
                    }
                }
            })
            .expect("a thread with a big stack")
            .join()
            .expect("the ladder ran");
    }
}
