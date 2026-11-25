//! Slow the top of a tall thin part, and adaptive layers on a curved top.

use std::collections::BTreeMap;

use super::geom::{contact, top_shape};
use super::{is_on, placed, setting, tip, Scene, Suggestion, SuggestionFix};

pub const TALL_MM: f64 = 50.0;
pub const TALL_ASPECT: f64 = 5.0;
pub const TOP_SPREAD_MM: f64 = 1.5;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for object in &scene.objects {
        let mesh = placed(object);
        let shape = contact(&mesh);
        let narrow = shape.base_x_mm.min(shape.base_y_mm).max(0.5);
        if shape.height_mm >= TALL_MM && shape.height_mm / narrow >= TALL_ASPECT {
            let mut settings = BTreeMap::new();
            settings.insert("speed_slowdown_layers".into(), "6".into());
            out.push(tip(
                "height_range",
                format!("height_range:slow:{}", object.id),
                "Slow the top of this tall part",
                format!(
                    "It is {:.0} mm tall on a {:.0} mm foot. The last few layers are where a tall thin part gets knocked, so those layers should slow down.",
                    shape.height_mm, narrow
                ),
                vec![object.id.clone()],
                Some(SuggestionFix {
                    label: "Slow the top layers".into(),
                    object_settings: BTreeMap::from([(object.id.clone(), settings)]),
                    ..SuggestionFix::default()
                }),
                38,
            ));
        }
        if is_on(setting(scene, object, "adaptive_layer_height")) {
            continue;
        }
        let top = top_shape(&mesh);
        if top.spread_mm >= TOP_SPREAD_MM && (1.5..12.0).contains(&top.max_slope_deg) {
            let mut settings = BTreeMap::new();
            settings.insert("adaptive_layer_height".into(), "true".into());
            out.push(tip(
                "height_range",
                format!("height_range:adaptive:{}", object.id),
                "Adaptive layers on the curved top",
                format!(
                    "The top rises {:.1} mm at up to {:.0}°. Adaptive layers keep the shallow slope smoother without slowing the straight walls.",
                    top.spread_mm, top.max_slope_deg
                ),
                vec![object.id.clone()],
                Some(SuggestionFix {
                    label: "Adaptive layers".into(),
                    object_settings: BTreeMap::from([(object.id.clone(), settings)]),
                    ..SuggestionFix::default()
                }),
                36,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::TriangleMesh;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};

    #[test]
    fn a_cube_is_neither_tall_nor_curved() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        assert!(check(&scene(vec![object("cube", mesh)])).is_empty());
    }

    #[test]
    fn a_tower_slows_the_top() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [8.0, 8.0, 80.0]);
        let found = check(&scene(vec![object("tower", mesh)]));
        assert!(
            found.iter().any(|item| item.id.contains("slow")),
            "{found:?}"
        );
    }

    #[test]
    fn a_ramp_asks_for_adaptive_layers() {
        let mesh = TriangleMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [40.0, 0.0, 0.0],
                [40.0, 40.0, 0.0],
                [0.0, 40.0, 0.0],
                [0.0, 0.0, 8.0],
                [40.0, 0.0, 8.0],
                [40.0, 40.0, 12.0],
                [0.0, 40.0, 12.0],
            ],
            triangles: vec![
                [0, 2, 1],
                [0, 3, 2],
                [4, 5, 6],
                [4, 6, 7],
                [0, 1, 5],
                [0, 5, 4],
                [3, 6, 2],
                [3, 7, 6],
                [0, 4, 7],
                [0, 7, 3],
                [1, 2, 6],
                [1, 6, 5],
            ],
        };
        let found = check(&scene(vec![object("ramp", mesh)]));
        assert!(
            found.iter().any(|item| item.id.contains("adaptive")),
            "{found:?}"
        );
    }
}
