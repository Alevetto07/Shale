//! Brim when the first layer is too small for the height.

use std::collections::BTreeMap;

use super::geom::contact;
use super::{placed, setting, tip, Scene, Suggestion, SuggestionFix};

/// Contact area below this fraction of height² is a small foot under a tall part.
/// A 20 mm cube has 400 mm² against 32 mm², so it stays quiet. An 8×8×80 mm
/// tower has 64 mm² against 512 mm², so it asks for a brim.
pub const CONTACT_PER_HEIGHT_SQ: f64 = 0.08;
/// Shorter than this, a small foot is just a short part.
pub const MIN_HEIGHT_MM: f64 = 18.0;
/// Height divided by the narrower base side.
pub const TALL_ASPECT: f64 = 4.0;
/// Contact divided by the bounding footprint, paired with [`TALL_ASPECT`].
pub const TALL_CONTACT_FRACTION: f64 = 0.35;
/// A contact island smaller than this, beside another island, is a toe.
pub const ISLAND_MM2: f64 = 6.0;
/// Centre of mass this far up the height, over a foot under [`TOP_HEAVY_CONTACT_MM2`].
pub const TOP_HEAVY_FRACTION: f64 = 0.62;
pub const TOP_HEAVY_CONTACT_MM2: f64 = 80.0;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for object in &scene.objects {
        let adhesion = setting(scene, object, "adhesion_type").unwrap_or("skirt");
        if adhesion.eq_ignore_ascii_case("brim") {
            continue;
        }
        let shape = contact(&placed(object));
        if shape.height_mm < MIN_HEIGHT_MM {
            continue;
        }
        let narrow = shape.base_x_mm.min(shape.base_y_mm).max(0.5);
        let aspect = shape.height_mm / narrow;
        let small_foot = shape.area_mm2 < CONTACT_PER_HEIGHT_SQ * shape.height_mm * shape.height_mm;
        let tall = aspect >= TALL_ASPECT
            && shape.footprint_mm2 > 0.0
            && shape.area_mm2 / shape.footprint_mm2 < TALL_CONTACT_FRACTION;
        let islands: Vec<f64> = shape
            .islands_mm2
            .iter()
            .copied()
            .filter(|area| *area >= 1.0)
            .collect();
        let toes = shape.height_mm >= 20.0
            && islands.len() >= 2
            && islands.iter().any(|area| *area < ISLAND_MM2);
        let top_heavy = shape.com_above_base_mm > TOP_HEAVY_FRACTION * shape.height_mm
            && shape.area_mm2 < TOP_HEAVY_CONTACT_MM2;
        if !(small_foot || tall || toes || top_heavy) {
            continue;
        }
        let width = if aspect >= 6.0 || shape.area_mm2 < TOP_HEAVY_CONTACT_MM2 {
            8.0
        } else {
            5.0
        };
        let mut settings = BTreeMap::new();
        settings.insert("adhesion_type".into(), "brim".into());
        settings.insert("brim_width".into(), format!("{width:.0}"));
        let why = if toes {
            format!(
                "The first layer is split into small islands, the smallest about {:.0} mm², under a {:.0} mm part.",
                islands.iter().copied().fold(f64::MAX, f64::min),
                shape.height_mm
            )
        } else if top_heavy && !small_foot {
            format!(
                "The weight sits {:.0} mm up and the first layer is only {:.0} mm².",
                shape.com_above_base_mm, shape.area_mm2
            )
        } else {
            format!(
                "The first layer is about {:.0} mm² under a {:.0} mm part, so a nudge can peel it off.",
                shape.area_mm2, shape.height_mm
            )
        };
        out.push(tip(
            "brim_contact",
            format!("brim_contact:{}", object.id),
            format!(
                "Add {} {width:.0} mm brim",
                if width < 8.0 { "a" } else { "an" }
            ),
            why,
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: format!("Add a {width:.0} mm brim"),
                settings,
                ..SuggestionFix::default()
            }),
            70,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};

    #[test]
    fn a_cube_does_not_ask_for_a_brim() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        let found = check(&scene(vec![object("cube", mesh)]));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn two_towers_become_one_brim_suggestion() {
        let scene = scene(vec![
            object("tower", box_mesh([0.0, 0.0, 0.0], [8.0, 8.0, 80.0])),
            object("mast", box_mesh([30.0, 0.0, 0.0], [36.0, 6.0, 90.0])),
        ]);
        let found = crate::suggest::evaluate(&scene);
        let brims: Vec<_> = found.iter().filter(|item| item.rule_id == "brim_contact").collect();
        assert_eq!(brims.len(), 1, "{found:?}");
    }

    #[test]
    fn a_tall_thin_tower_asks_for_a_brim() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [8.0, 8.0, 80.0]);
        let found = check(&scene(vec![object("tower", mesh)]));
        assert_eq!(found.len(), 1, "{found:?}");
        let fix = found[0].fix.as_ref().unwrap();
        assert_eq!(
            fix.settings.get("adhesion_type").map(String::as_str),
            Some("brim")
        );
        assert_eq!(
            fix.settings.get("brim_width").map(String::as_str),
            Some("8")
        );
    }

    #[test]
    fn an_existing_brim_is_left_alone() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [8.0, 8.0, 80.0]);
        let mut plate = scene(vec![object("tower", mesh)]);
        plate.settings.insert("adhesion_type".into(), "brim".into());
        assert!(check(&plate).is_empty());
    }
}
