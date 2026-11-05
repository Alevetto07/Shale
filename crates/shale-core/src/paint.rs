//! Painting supports.
//!
//! Bambu Studio lets you paint the faces that need support. The engine has no
//! concept of a painted face, but it does take `support_mesh` volumes: a volume
//! marked that way is filled with support and, when the plate asks for no
//! support at all, it is the only place support is generated.
//!
//! So painting here is two steps. The downward faces of a part are grouped into
//! clusters — one cluster is one contiguous patch of underside, the thing you
//! would drag a brush across — and a cluster becomes a column of prisms: every
//! painted triangle extruded straight down to the bed. Prisms that share an
//! edge overlap at the seam, which CuraEngine does not mind: point-in-mesh is
//! what it asks of a modifier.
//!
//! Nothing here touches the part. The part keeps its triangles and its volume;
//! the prisms are a second mesh that goes to the engine as a modifier.

use serde::{Deserialize, Serialize};

use crate::mesh::{signed_volume, TriangleMesh};

/// Cura's `support_angle`, measured from the vertical: a face that leans more
/// than this towards the bed needs something under it, and a wall is its own
/// support. A horizontal ceiling leans all the way, so it always qualifies.
pub const PAINT_ANGLE_DEG: f64 = 50.0;

/// How far apart two underside triangles can be and still be one patch. An STL
/// carries no shared vertices, so adjacency is a matter of distance, not
/// topology.
pub const PAINT_REACH_MM: f64 = 8.0;

/// A patch whose highest point is below this is lying on the bed, not hanging
/// over it. It is left out of the list of things to paint.
pub const BED_CLEARANCE_MM: f64 = 0.6;

/// Patches smaller than this are not worth a row in a list.
pub const MIN_PATCH_MM2: f64 = 4.0;

/// One contiguous patch of underside.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Patch {
    /// The triangles that belong to this patch, in mesh order.
    pub triangles: Vec<usize>,
    pub area_mm2: f64,
    /// The patch's own bounding box, world space.
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// A painted patch on one of the plate's parts, ready for the engine.
///
/// `model` is the part's position in the plate's model list — the engine is
/// given the parts in that order — and `triangles` index that part's own mesh,
/// which is the mesh the patches were found on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaintedPatch {
    pub model: usize,
    pub triangles: Vec<usize>,
}

fn normal(mesh: &TriangleMesh, index: usize) -> [f64; 3] {
    let [a, b, c] = triangle_points(mesh, index);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        (u[1] as f64) * (v[2] as f64) - (u[2] as f64) * (v[1] as f64),
        (u[2] as f64) * (v[0] as f64) - (u[0] as f64) * (v[2] as f64),
        (u[0] as f64) * (v[1] as f64) - (u[1] as f64) * (v[0] as f64),
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if length <= f64::EPSILON {
        return [0.0, 0.0, 0.0];
    }
    [
        cross[0] / length,
        cross[1] / length,
        cross[2] / length,
    ]
}

fn triangle_points(mesh: &TriangleMesh, index: usize) -> [[f32; 3]; 3] {
    let corners = mesh.triangles[index];
    [
        mesh.vertices[corners[0] as usize],
        mesh.vertices[corners[1] as usize],
        mesh.vertices[corners[2] as usize],
    ]
}

fn triangle_area(points: &[[f32; 3]; 3]) -> f64 {
    let u = [
        points[1][0] as f64 - points[0][0] as f64,
        points[1][1] as f64 - points[0][1] as f64,
        points[1][2] as f64 - points[0][2] as f64,
    ];
    let v = [
        points[2][0] as f64 - points[0][0] as f64,
        points[2][1] as f64 - points[0][1] as f64,
        points[2][2] as f64 - points[0][2] as f64,
    ];
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt()
}

/// The triangles a brush would be allowed to paint: facing down, and flat
/// enough that the engine would want something under them.
pub fn paintable(mesh: &TriangleMesh, angle_deg: f64) -> Vec<usize> {
    let limit = angle_deg.clamp(0.0, 90.0);
    let threshold = limit.to_radians().cos();
    (0..mesh.triangles.len())
        .filter(|index| {
            let n = normal(mesh, *index);
            // `-n.z` is one for a ceiling and zero for a wall, so a bigger
            // support angle asks for a flatter underside.
            -n[2] >= threshold && n[2] < -0.01
        })
        .collect()
}

/// Group the paintable triangles into contiguous patches.
///
/// Two triangles are neighbours when their centres are within `reach_mm` in the
/// plane and their heights overlap. A cantilever whose underside is two
/// triangles is one patch; the far cantilever is another.
pub fn patches(mesh: &TriangleMesh, angle_deg: f64, reach_mm: f64) -> Vec<Patch> {
    let faces = paintable(mesh, angle_deg);
    if faces.is_empty() {
        return Vec::new();
    }
    let reach = reach_mm.max(0.5);
    let mut parent: Vec<usize> = (0..faces.len()).collect();

    fn root(parent: &mut [usize], mut index: usize) -> usize {
        while parent[index] != index {
            parent[index] = parent[parent[index]];
            index = parent[index];
        }
        index
    }

    let centres: Vec<[f64; 3]> = faces
        .iter()
        .map(|index| {
            let points = triangle_points(mesh, *index);
            [
                (points[0][0] as f64 + points[1][0] as f64 + points[2][0] as f64) / 3.0,
                (points[0][1] as f64 + points[1][1] as f64 + points[2][1] as f64) / 3.0,
                (points[0][2] as f64 + points[1][2] as f64 + points[2][2] as f64) / 3.0,
            ]
        })
        .collect();

    // A bucket grid keeps this linear on a real mesh instead of comparing every
    // pair of a 100 000-triangle part.
    let mut buckets: std::collections::BTreeMap<(i32, i32), Vec<usize>> =
        std::collections::BTreeMap::new();
    let cell = reach;
    let key = |point: &[f64; 3]| ((point[0] / cell).floor() as i32, (point[1] / cell).floor() as i32);
    for (index, centre) in centres.iter().enumerate() {
        buckets.entry(key(centre)).or_default().push(index);
    }
    for (index, centre) in centres.iter().enumerate() {
        let (ix, iy) = key(centre);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(neighbours) = buckets.get(&(ix + dx, iy + dy)) {
                    for other in neighbours {
                        if *other <= index {
                            continue;
                        }
                        let there = centres[*other];
                        let planar = ((there[0] - centre[0]).powi(2)
                            + (there[1] - centre[1]).powi(2))
                        .sqrt();
                        if planar <= reach && (there[2] - centre[2]).abs() <= 2.0 {
                            let a = root(&mut parent, index);
                            let b = root(&mut parent, *other);
                            if a != b {
                                parent[a] = b;
                            }
                        }
                    }
                }
            }
        }
    }

    // An indexed mesh (a duplicate, a 3MF) shares vertices, and two triangles
    // on one edge are neighbours however long that edge is. An STL writes every
    // corner out three times over, so the corners are welded by position first:
    // without that, the two halves of one flat ceiling look like two patches
    // because their own vertices are not the same numbers.
    let mut welded: std::collections::BTreeMap<(i64, i64, i64), u32> =
        std::collections::BTreeMap::new();
    let mut corner_id = |point: [f32; 3]| -> u32 {
        let key = (
            (point[0] as f64 * 1000.0).round() as i64,
            (point[1] as f64 * 1000.0).round() as i64,
            (point[2] as f64 * 1000.0).round() as i64,
        );
        let next = welded.len() as u32;
        *welded.entry(key).or_insert(next)
    };
    let mut edge_owner: std::collections::BTreeMap<(u32, u32), Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, face) in faces.iter().enumerate() {
        let corners = mesh.triangles[*face];
        let ids = [
            corner_id(mesh.vertices[corners[0] as usize]),
            corner_id(mesh.vertices[corners[1] as usize]),
            corner_id(mesh.vertices[corners[2] as usize]),
        ];
        for pair in [[0usize, 1], [1, 2], [2, 0]] {
            let a = ids[pair[0]];
            let b = ids[pair[1]];
            let edge = if a < b { (a, b) } else { (b, a) };
            edge_owner.entry(edge).or_default().push(index);
        }
    }
    for owners in edge_owner.values() {
        for pair in owners.windows(2) {
            let a = root(&mut parent, pair[0]);
            let b = root(&mut parent, pair[1]);
            if a != b {
                parent[a] = b;
            }
        }
    }

    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for index in 0..faces.len() {
        let root = root(&mut parent, index);
        groups.entry(root).or_default().push(index);
    }

    let mut patches: Vec<Patch> = groups
        .into_values()
        .map(|members| {
            let mut triangles = Vec::new();
            let mut area = 0.0;
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for member in members {
                let face = faces[member];
                triangles.push(face);
                let points = triangle_points(mesh, face);
                area += triangle_area(&points);
                for point in points {
                    for axis in 0..3 {
                        min[axis] = min[axis].min(point[axis] as f64);
                        max[axis] = max[axis].max(point[axis] as f64);
                    }
                }
            }
            triangles.sort_unstable();
            Patch {
                triangles,
                area_mm2: area,
                min,
                max,
            }
        })
        // A face lying on the bed needs nothing under it: there is no gap for a
        // column to fill, and offering it as paintable would grow a zero-height
        // column the engine has to be told to ignore.
        .filter(|patch| patch.area_mm2 >= MIN_PATCH_MM2 && patch.max[2] > BED_CLEARANCE_MM)
        .collect();
    // Biggest first: that is the patch a person is most likely to mean.
    patches.sort_by(|a, b| {
        b.area_mm2
            .partial_cmp(&a.area_mm2)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    patches
}

/// A column of prisms: every painted triangle, extruded down to `floor_mm`.
///
/// Each prism is closed and wound outward on its own, so the engine's
/// point-in-mesh test works whether the painted faces are one triangle or a
/// thousand, and adjacent prisms simply overlap at the seam.
pub fn prism_mesh(mesh: &TriangleMesh, triangles: &[usize], floor_mm: f64) -> TriangleMesh {
    let mut out = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    for index in triangles {
        let Some(corners) = mesh.triangles.get(*index) else {
            continue;
        };
        let points = [
            mesh.vertices[corners[0] as usize],
            mesh.vertices[corners[1] as usize],
            mesh.vertices[corners[2] as usize],
        ];
        let top = points.map(|point| [point[0], point[1], point[2]]);
        if top.iter().all(|point| (point[2] as f64) <= floor_mm + 1e-6) {
            // Already on the bed: there is nothing to hold up.
            continue;
        }
        let bottom = top.map(|point| [point[0], point[1], floor_mm as f32]);

        let mut prism = TriangleMesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        for point in [top[0], top[1], top[2], bottom[0], bottom[1], bottom[2]] {
            prism.vertices.push(point);
        }
        // 0..3 top, 3..6 bottom
        let faces: [[u32; 3]; 8] = [
            [0, 1, 2], // top
            [3, 5, 4], // bottom, wound the other way
            [0, 3, 4],
            [0, 4, 1],
            [1, 4, 5],
            [1, 5, 2],
            [2, 5, 3],
            [2, 3, 0],
        ];
        for face in faces {
            prism.triangles.push(face);
        }
        // A prism built from a triangle whose winding went the other way comes
        // out inside out; the signed volume is the cheapest way to know.
        if signed_volume(&prism) < 0.0 {
            for triangle in &mut prism.triangles {
                triangle.swap(1, 2);
            }
        }
        let offset = out.vertices.len() as u32;
        out.vertices.extend(prism.vertices);
        for triangle in prism.triangles {
            out.triangles
                .push([triangle[0] + offset, triangle[1] + offset, triangle[2] + offset]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(vertices: Vec<[f32; 3]>, triangles: Vec<[u32; 3]>) -> TriangleMesh {
        TriangleMesh { vertices, triangles }
    }

    /// One downward-facing triangle, ten millimetres up.
    fn lid() -> TriangleMesh {
        mesh(
            vec![
                [0.0, 0.0, 10.0],
                [20.0, 0.0, 10.0],
                [0.0, 10.0, 10.0],
            ],
            vec![[2, 1, 0]],
        )
    }

    #[test]
    fn a_downward_face_can_be_painted_and_a_wall_cannot() {
        let flat = lid();
        assert_eq!(paintable(&flat, PAINT_ANGLE_DEG), vec![0]);

        // The same triangle wound the other way faces up: not an underside.
        let up = mesh(
            flat.vertices.clone(),
            vec![[0, 1, 2]],
        );
        assert!(paintable(&up, PAINT_ANGLE_DEG).is_empty());

        // A wall is vertical, which is 90 degrees from vertical: its own support.
        let wall = mesh(
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 10.0],
            ],
            vec![[2, 1, 0]],
        );
        assert!(paintable(&wall, PAINT_ANGLE_DEG).is_empty());
    }

    #[test]
    fn two_cantilevers_are_two_patches() {
        // Two 20x10 lids, 40 mm apart, both facing down.
        let split = mesh(
            vec![
                [0.0, 0.0, 10.0],
                [20.0, 0.0, 10.0],
                [0.0, 10.0, 10.0],
                [40.0, 0.0, 10.0],
                [60.0, 0.0, 10.0],
                [40.0, 10.0, 10.0],
            ],
            vec![[2, 1, 0], [5, 4, 3]],
        );
        let patches = patches(&split, PAINT_ANGLE_DEG, PAINT_REACH_MM);
        assert_eq!(patches.len(), 2, "one patch per cantilever");
        assert!((patches[0].area_mm2 - 100.0).abs() < 0.01);
        assert!((patches[1].area_mm2 - 100.0).abs() < 0.01);
    }

    #[test]
    fn a_shared_edge_is_one_patch() {
        // Two halves of the same 20x10 lid, sharing an edge: one brush stroke.
        let shared = mesh(
            vec![
                [0.0, 0.0, 10.0],
                [20.0, 0.0, 10.0],
                [0.0, 10.0, 10.0],
                [20.0, 10.0, 10.0],
            ],
            vec![[2, 1, 0], [3, 1, 2]],
        );
        let patches = patches(&shared, PAINT_ANGLE_DEG, PAINT_REACH_MM);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].triangles.len(), 2);
        // Two right triangles, ten by twenty each: two hundred square
        // millimetres between them.
        assert!((patches[0].area_mm2 - 200.0).abs() < 0.01);
    }

    #[test]
    fn a_painted_triangle_becomes_a_closed_prism() {
        let flat = lid();
        let prisms = prism_mesh(&flat, &[0], 0.0);
        assert_eq!(prisms.triangles.len(), 8);
        assert_eq!(prisms.vertices.len(), 6);
        let lowest = prisms
            .vertices
            .iter()
            .map(|point| point[2])
            .fold(f32::INFINITY, f32::min);
        assert_eq!(lowest, 0.0, "the column reaches the bed");

        // A right triangle 20 x 10 is a hundred square millimetres, ten tall:
        // a thousand cubic millimetres, and it has to come out the right way
        // round, or the engine reads the column as a hole.
        let volume = signed_volume(&prisms);
        assert!((volume - 1000.0).abs() < 0.01, "volume was {volume}");
    }

    #[test]
    fn a_face_on_the_bed_makes_no_prism() {
        let flat = mesh(
            vec![[0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            vec![[2, 1, 0]],
        );
        assert!(prism_mesh(&flat, &[0], 0.0).triangles.is_empty());
    }

    /// An STL writes each corner out again for every triangle it belongs to, so
    /// the two halves of one ceiling arrive with different vertex numbers. They
    /// are still one ceiling, and a person painting it means the whole of it.
    #[test]
    fn one_ceiling_written_out_twice_is_still_one_patch() {
        let corners = [
            [0.0, 0.0, 10.0],
            [20.0, 0.0, 10.0],
            [20.0, 10.0, 10.0],
            [0.0, 10.0, 10.0],
        ];
        let mut vertices = Vec::new();
        for corner in corners {
            vertices.push(corner);
        }
        // the second triangle is the same ceiling written out again: it shares
        // the diagonal with the first but none of its vertex numbers
        vertices.extend_from_slice(&[[0.0, 0.0, 10.0], [0.0, 10.0, 10.0], [20.0, 10.0, 10.0]]);
        let mesh = mesh(vertices, vec![[2, 1, 0], [4, 5, 6]]);
        let found = patches(&mesh, PAINT_ANGLE_DEG, PAINT_REACH_MM);
        assert_eq!(found.len(), 1, "one ceiling, one patch: {:?}", found.len());
        assert!((found[0].area_mm2 - 200.0).abs() < 0.01);
    }

    /// A face lying on the bed has no gap under it, so there is nothing a
    /// column could hold up and nothing to paint.
    #[test]
    fn a_face_lying_on_the_bed_is_not_offered() {
        let mesh = mesh(
            vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [10.0, 10.0, 0.0],
                [0.0, 10.0, 0.0],
            ],
            vec![[0, 2, 1], [0, 3, 2]],
        );
        assert!(patches(&mesh, PAINT_ANGLE_DEG, PAINT_REACH_MM).is_empty());
    }

}
