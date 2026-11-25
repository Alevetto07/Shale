//! Hide the seam on a sharp edge, or off an obvious front face.

use std::collections::BTreeMap;

use super::geom::bounds;
use super::{placed, setting, tip, Scene, Suggestion, SuggestionFix};

/// Normals this far apart are sharper than a cube's right angle.
const SHARP_DOT: f64 = -0.17;
const MIN_EDGE_MM: f64 = 8.0;
/// One face this many times the opposite face is "the front".
const FRONT_RATIO: f64 = 2.2;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for object in &scene.objects {
        let mesh = placed(object);
        let already = setting(scene, object, "z_seam_type").unwrap_or("");
        if let Some(position) = obvious_front(&mesh) {
            if already.eq_ignore_ascii_case("back") {
                continue;
            }
            let mut settings = BTreeMap::new();
            settings.insert("z_seam_type".into(), "back".into());
            settings.insert("z_seam_position".into(), position.into());
            out.push(tip(
                "seam",
                format!("seam:{}", object.id),
                "Put the seam on the back",
                "One flat face is much larger than the one opposite it. Starting the seam on the other side leaves that face clean.",
                vec![object.id.clone()],
                Some(SuggestionFix {
                    label: "Seam on the back".into(),
                    object_settings: BTreeMap::from([(object.id.clone(), settings)]),
                    ..SuggestionFix::default()
                }),
                40,
            ));
            continue;
        }
        if sharp_edge(&mesh) && !already.eq_ignore_ascii_case("sharpest_corner") {
            let mut settings = BTreeMap::new();
            settings.insert("z_seam_type".into(), "sharpest_corner".into());
            out.push(tip(
                "seam",
                format!("seam:{}", object.id),
                "Hide the seam on a sharp edge",
                "A vertical edge is sharper than a square corner, so the seam can sit in it instead of on a flat face.",
                vec![object.id.clone()],
                Some(SuggestionFix {
                    label: "Sharpest corner".into(),
                    object_settings: BTreeMap::from([(object.id.clone(), settings)]),
                    ..SuggestionFix::default()
                }),
                35,
            ));
        }
    }
    out
}

fn obvious_front(mesh: &crate::mesh::TriangleMesh) -> Option<&'static str> {
    let mut area = [0.0; 4];
    for triangle in &mesh.triangles {
        let Some(face) = face_of(mesh, triangle) else {
            continue;
        };
        if face.normal[2].abs() > 0.4 {
            continue;
        }
        let bin = if face.normal[0].abs() >= face.normal[1].abs() {
            if face.normal[0] >= 0.0 {
                0
            } else {
                1
            }
        } else if face.normal[1] >= 0.0 {
            2
        } else {
            3
        };
        area[bin] += face.area;
    }
    let pairs = [
        (0, 1, "left"),
        (1, 0, "right"),
        (2, 3, "front"),
        (3, 2, "back"),
    ];
    let mut best: Option<(f64, &'static str)> = None;
    for (bin, opposite, position) in pairs {
        if area[opposite] <= 1.0 {
            if area[bin] > 40.0 {
                return Some(position);
            }
            continue;
        }
        let ratio = area[bin] / area[opposite];
        if ratio >= FRONT_RATIO && best.is_none_or(|(current, _)| ratio > current) {
            best = Some((ratio, position));
        }
    }
    best.map(|(_, position)| position)
}

fn sharp_edge(mesh: &crate::mesh::TriangleMesh) -> bool {
    let mut edges: BTreeMap<(u32, u32), Vec<[f64; 3]>> = BTreeMap::new();
    for triangle in &mesh.triangles {
        let Some(face) = face_of(mesh, triangle) else {
            continue;
        };
        if face.normal[2].abs() > 0.45 {
            continue;
        }
        for edge in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let key = if edge.0 < edge.1 {
                (edge.0, edge.1)
            } else {
                (edge.1, edge.0)
            };
            edges.entry(key).or_default().push(face.normal);
        }
    }
    let (min, max) = bounds(mesh);
    let height = max[2] - min[2];
    for ((a, b), normals) in edges {
        if normals.len() < 2 {
            continue;
        }
        let Some(pa) = mesh.vertices.get(a as usize) else {
            continue;
        };
        let Some(pb) = mesh.vertices.get(b as usize) else {
            continue;
        };
        let dz = (pa[2] - pb[2]).abs() as f64;
        let dx = (pa[0] - pb[0]).abs() as f64;
        let dy = (pa[1] - pb[1]).abs() as f64;
        let length = (dx * dx + dy * dy + dz * dz).sqrt();
        if dz < length * 0.7 || length < MIN_EDGE_MM.min(height * 0.4) {
            continue;
        }
        let dot = normals[0][0] * normals[1][0]
            + normals[0][1] * normals[1][1]
            + normals[0][2] * normals[1][2];
        if dot < SHARP_DOT {
            return true;
        }
    }
    false
}

struct Face {
    normal: [f64; 3],
    area: f64,
}

fn face_of(mesh: &crate::mesh::TriangleMesh, triangle: &[u32; 3]) -> Option<Face> {
    let a = vert(mesh, triangle[0])?;
    let b = vert(mesh, triangle[1])?;
    let c = vert(mesh, triangle[2])?;
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if length < 1e-8 {
        return None;
    }
    Some(Face {
        normal: [cross[0] / length, cross[1] / length, cross[2] / length],
        area: length * 0.5,
    })
}

fn vert(mesh: &crate::mesh::TriangleMesh, index: u32) -> Option<[f64; 3]> {
    mesh.vertices
        .get(index as usize)
        .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::TriangleMesh;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};

    #[test]
    fn a_cube_has_no_obvious_front() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        assert!(check(&scene(vec![object("cube", mesh)])).is_empty());
    }

    #[test]
    fn a_wedge_hides_the_seam_off_the_flat_face() {
        let mut mesh = TriangleMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [30.0, 0.0, 0.0],
                [30.0, 16.0, 0.0],
                [0.0, 16.0, 0.0],
                [0.0, 0.0, 24.0],
                [30.0, 0.0, 24.0],
                [30.0, 16.0, 4.0],
                [0.0, 16.0, 4.0],
            ],
            triangles: Vec::new(),
        };
        // Bottom, sloped top, vertical front (y = 0), two sides. No vertical back.
        let faces: [[u32; 3]; 10] = [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [0, 4, 7],
            [0, 7, 3],
            [1, 2, 6],
            [1, 6, 5],
        ];
        mesh.triangles.extend_from_slice(&faces);
        let found = check(&scene(vec![object("wedge", mesh)]));
        assert!(found.iter().any(|item| item.rule_id == "seam"), "{found:?}");
    }
}
