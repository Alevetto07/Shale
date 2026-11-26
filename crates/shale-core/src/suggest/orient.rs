//! A quarter-turn when it cuts support or height by more than about 20%.

use std::collections::BTreeMap;

use crate::mesh::TriangleMesh;
use crate::orient::{best_orientation, overhang_area};
use crate::Transform;

use super::geom::bounds;
use super::{tip, Scene, Suggestion, SuggestionFix};

/// Saving below this stays a guess, not a suggestion.
pub const MIN_SAVING: f64 = 0.20;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for object in &scene.objects {
        if object.mesh.vertices.len() < 4 {
            continue;
        }
        let current = score(&object.mesh, object.transform.rotation_deg);
        let best = best_orientation(&object.mesh, 50.0);
        let best_proxy = proxy(best.height_mm, best.overhang_mm2, best.footprint_mm2);
        if current.proxy <= 0.0 || best_proxy >= current.proxy * (1.0 - MIN_SAVING) {
            continue;
        }
        if same_turn(object.transform.rotation_deg, best.rotation_deg) {
            continue;
        }
        let saving = (1.0 - best_proxy / current.proxy) * 100.0;
        let reason = if current.overhang > best.overhang_mm2 + 1.0 {
            format!(
                "Support area falls from {:.0} mm² to {:.0} mm², about {:.0}% less to print.",
                current.overhang, best.overhang_mm2, saving
            )
        } else {
            format!(
                "The part stands {:.0} mm tall and would stand {:.0} mm, about {:.0}% less print time.",
                current.height, best.height_mm, saving
            )
        };
        out.push(tip(
            "auto_orient",
            format!("auto_orient:{}", object.id),
            format!("Turn it, about {saving:.0}% less"),
            reason,
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: "Turn the part".into(),
                rotations_deg: BTreeMap::from([(object.id.clone(), best.rotation_deg)]),
                ..SuggestionFix::default()
            }),
            60,
        ));
    }
    out
}

struct Score {
    height: f64,
    overhang: f64,
    proxy: f64,
}

fn score(mesh: &TriangleMesh, rotation: [f64; 3]) -> Score {
    let shaped = crate::mesh::bake(
        mesh,
        &Transform {
            translation: [0.0, 0.0, 0.0],
            rotation_deg: rotation,
            scale: [1.0, 1.0, 1.0],
        },
    );
    let (min, max) = bounds(&shaped);
    let height = (max[2] - min[2]).max(0.0);
    let footprint = (max[0] - min[0]).max(0.0) * (max[1] - min[1]).max(0.0);
    let overhang = overhang_area(mesh, rotation, 50.0);
    Score {
        height,
        overhang,
        proxy: proxy(height, overhang, footprint),
    }
}

fn proxy(height: f64, overhang: f64, footprint: f64) -> f64 {
    height * (1.0 + overhang / footprint.max(1.0))
}

fn same_turn(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter()
        .zip(b)
        .all(|(left, right)| angle_delta(*left, right) < 2.0)
}

fn angle_delta(a: f64, b: f64) -> f64 {
    let mut delta = (a - b).rem_euclid(360.0);
    if delta > 180.0 {
        delta = 360.0 - delta;
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};

    #[test]
    fn a_cube_is_already_fine() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        assert!(check(&scene(vec![object("cube", mesh)])).is_empty());
    }

    #[test]
    fn a_bar_standing_on_end_is_worth_laying_down() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [40.0, 10.0, 10.0]);
        let mut part = object("bar", mesh);
        part.transform.rotation_deg = [0.0, 90.0, 0.0];
        let found = check(&scene(vec![part]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].title.contains('%'), "{}", found[0].title);
        assert!(found[0].reason.contains("less"));
    }
}
