//! Brim ears: small discs at the sharp corners of a part with a small foot.
//!
//! Bambu Studio calls them mouse ears. CuraEngine has a brim for the whole
//! first layer and no ear mode, so the discs are ordinary meshes, one layer
//! tall, added beside the part. A round foot has no sharp corner and grows
//! nothing. A large foot is left alone unless the object asks for ears.

use crate::mesh::{bake, Transform, TriangleMesh};
use crate::primitives::cylinder;

/// A corner this sharp, or sharper, can take an ear. 90° is a square corner.
const SHARP_DEG: f64 = 125.0;
/// Footprint under this is a small part.
const SMALL_AREA_MM2: f64 = 200.0;
/// A narrow side under this, on a footprint that is not already huge.
const NARROW_MM: f64 = 12.0;
const NARROW_AREA_MM2: f64 = 800.0;
const DEFAULT_RADIUS_MM: f64 = 5.0;
/// How far the disc centre sits outside the corner, as a fraction of the radius.
/// The rest of the disc overlaps the part, so the ear is attached.
const OUTSET: f64 = 0.55;

/// Keys the panel stores on an object. They are not Cura settings.
pub fn is_private(key: &str) -> bool {
    key == "brim_ears" || key == "brim_ear_radius"
}

#[derive(Clone, Debug)]
pub struct Ear {
    pub mesh: TriangleMesh,
    pub settings: Vec<(String, String)>,
}

/// Discs for one object. Empty when the object opts out, the plate already has
/// a brim and the object did not force ears, or the foot has no sharp corner.
pub fn ears_for(
    mesh: &TriangleMesh,
    transform: &Transform,
    settings: &[(String, String)],
    layer_height: f64,
    plate_has_brim: bool,
) -> Vec<Ear> {
    let mode = ear_mode(settings);
    if mode == EarMode::Off {
        return Vec::new();
    }
    if mode == EarMode::Auto && plate_has_brim {
        return Vec::new();
    }
    let world = bake(mesh, transform);
    let (min, _) = world.bounds();
    let floor = f64::from(min[2]);
    let mut points = Vec::new();
    for vertex in &world.vertices {
        if f64::from(vertex[2]) <= floor + 0.4 {
            points.push([f64::from(vertex[0]), f64::from(vertex[1])]);
        }
    }
    let hull = convex_hull(&mut points);
    if hull.len() < 3 {
        return Vec::new();
    }
    if mode == EarMode::Auto && !small_foot(&hull) {
        return Vec::new();
    }
    let radius = ear_radius(settings);
    let height = layer_height.clamp(0.08, 0.4);
    let mut ears = Vec::new();
    let count = hull.len();
    for index in 0..count {
        let previous = hull[(index + count - 1) % count];
        let here = hull[index];
        let next = hull[(index + 1) % count];
        let angle = corner_angle(previous, here, next);
        if angle > SHARP_DEG {
            continue;
        }
        let outward = outward_bisector(previous, here, next);
        let centre = [
            here[0] + outward[0] * radius * OUTSET,
            here[1] + outward[1] * radius * OUTSET,
        ];
        if ears.iter().any(|ear: &Ear| {
            let (low, high) = ear.mesh.bounds();
            let x = f64::from(low[0] + high[0]) * 0.5;
            let y = f64::from(low[1] + high[1]) * 0.5;
            let dx = x - centre[0];
            let dy = y - centre[1];
            (dx * dx + dy * dy).sqrt() < radius * 0.75
        }) {
            continue;
        }
        ears.push(Ear {
            mesh: disc(centre, floor, radius, height),
            settings: ear_settings(),
        });
        if ears.len() == 12 {
            break;
        }
    }
    ears
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EarMode {
    Auto,
    On,
    Off,
}

fn setting<'a>(settings: &'a [(String, String)], key: &str) -> Option<&'a str> {
    settings
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn ear_mode(settings: &[(String, String)]) -> EarMode {
    match setting(settings, "brim_ears")
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "auto" => EarMode::Auto,
        "0" | "false" | "off" | "no" => EarMode::Off,
        _ => EarMode::On,
    }
}

fn ear_radius(settings: &[(String, String)]) -> f64 {
    setting(settings, "brim_ear_radius")
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_RADIUS_MM)
        .clamp(1.5, 20.0)
}

fn ear_settings() -> Vec<(String, String)> {
    vec![
        ("wall_line_count".into(), "1".into()),
        ("top_layers".into(), "0".into()),
        ("bottom_layers".into(), "1".into()),
        ("infill_sparse_density".into(), "100".into()),
        ("support_enable".into(), "0".into()),
        ("adhesion_type".into(), "none".into()),
    ]
}

fn disc(centre: [f64; 2], z: f64, radius: f64, height: f64) -> TriangleMesh {
    let mut mesh = cylinder(radius, height);
    for vertex in &mut mesh.vertices {
        vertex[0] += centre[0] as f32;
        vertex[1] += centre[1] as f32;
        vertex[2] += z as f32;
    }
    mesh
}

fn small_foot(hull: &[[f64; 2]]) -> bool {
    let area = polygon_area(hull).abs();
    let (min, max) = bounds2(hull);
    let narrow = (max[0] - min[0]).min(max[1] - min[1]);
    area < SMALL_AREA_MM2 || (narrow < NARROW_MM && area < NARROW_AREA_MM2)
}

fn polygon_area(hull: &[[f64; 2]]) -> f64 {
    let mut area = 0.0;
    for index in 0..hull.len() {
        let here = hull[index];
        let next = hull[(index + 1) % hull.len()];
        area += here[0] * next[1] - next[0] * here[1];
    }
    area * 0.5
}

fn bounds2(points: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for point in points {
        min[0] = min[0].min(point[0]);
        min[1] = min[1].min(point[1]);
        max[0] = max[0].max(point[0]);
        max[1] = max[1].max(point[1]);
    }
    (min, max)
}

/// Interior angle at `here`, in degrees.
fn corner_angle(previous: [f64; 2], here: [f64; 2], next: [f64; 2]) -> f64 {
    let left = normalise2([previous[0] - here[0], previous[1] - here[1]]);
    let right = normalise2([next[0] - here[0], next[1] - here[1]]);
    let dot = (left[0] * right[0] + left[1] * right[1]).clamp(-1.0, 1.0);
    dot.acos().to_degrees()
}

/// Unit vector pointing out of a counter-clockwise hull at `here`.
fn outward_bisector(previous: [f64; 2], here: [f64; 2], next: [f64; 2]) -> [f64; 2] {
    let left = normalise2([previous[0] - here[0], previous[1] - here[1]]);
    let right = normalise2([next[0] - here[0], next[1] - here[1]]);
    let inward = normalise2([left[0] + right[0], left[1] + right[1]]);
    [-inward[0], -inward[1]]
}

fn normalise2(point: [f64; 2]) -> [f64; 2] {
    let length = (point[0] * point[0] + point[1] * point[1]).sqrt();
    if length < 1e-9 {
        [1.0, 0.0]
    } else {
        [point[0] / length, point[1] / length]
    }
}

fn convex_hull(points: &mut Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.sort_by(|a, b| {
        a[0].partial_cmp(&b[0])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a[1].partial_cmp(&b[1]).unwrap_or(std::cmp::Ordering::Equal))
    });
    points.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4);
    if points.len() <= 2 {
        return points.clone();
    }
    let cross = |origin: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - origin[0]) * (b[1] - origin[1]) - (a[1] - origin[1]) * (b[0] - origin[0])
    };
    let mut lower = Vec::new();
    for point in points.iter().copied() {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], point) <= 0.0
        {
            lower.pop();
        }
        lower.push(point);
    }
    let mut upper = Vec::new();
    for point in points.iter().rev().copied() {
        while upper.len() >= 2
            && cross(upper[upper.len() - 2], upper[upper.len() - 1], point) <= 0.0
        {
            upper.pop();
        }
        upper.push(point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{box_solid, cylinder};

    fn identity() -> Transform {
        Transform::default()
    }

    #[test]
    fn a_small_square_foot_grows_an_ear_at_each_corner() {
        let mesh = box_solid(8.0, 8.0, 40.0);
        let ears = ears_for(&mesh, &identity(), &[], 0.2, false);
        assert_eq!(ears.len(), 4, "four sharp corners");
        for ear in &ears {
            let (min, max) = ear.mesh.bounds();
            assert!(f64::from(min[2]).abs() < 1e-3, "on the bed: {min:?}");
            assert!((f64::from(max[2] - min[2]) - 0.2).abs() < 1e-3);
        }
    }

    #[test]
    fn a_large_cube_is_left_alone_unless_asked() {
        let mesh = box_solid(30.0, 30.0, 30.0);
        assert!(ears_for(&mesh, &identity(), &[], 0.2, false).is_empty());
        let forced = ears_for(
            &mesh,
            &identity(),
            &[("brim_ears".into(), "1".into())],
            0.2,
            false,
        );
        assert_eq!(forced.len(), 4);
    }

    #[test]
    fn a_round_foot_has_no_sharp_corner() {
        let mesh = cylinder(4.0, 40.0);
        assert!(
            ears_for(&mesh, &identity(), &[], 0.2, false).is_empty(),
            "a circle is not a corner"
        );
    }

    #[test]
    fn an_object_can_refuse_ears_and_a_plate_brim_already_covers_them() {
        let mesh = box_solid(8.0, 8.0, 40.0);
        let off = ears_for(
            &mesh,
            &identity(),
            &[("brim_ears".into(), "0".into())],
            0.2,
            false,
        );
        assert!(off.is_empty());
        assert!(ears_for(&mesh, &identity(), &[], 0.2, true).is_empty());
    }
}
