//! Flush purge into infill when more than one colour is really in use.

use std::collections::BTreeMap;

use super::{tip, Scene, Suggestion, SuggestionFix};

/// Grams of a typical AMS flush that infill can absorb.
pub const GRAMS_SAVED_PER_CHANGE: f64 = 0.25;
pub const MIN_SAVED_G: f64 = 0.2;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    if !scene.printer.multi_material {
        return Vec::new();
    }
    if super::is_on(setting_plate(scene, "flush_into_infill")) {
        return Vec::new();
    }
    let mut used = BTreeMap::<usize, ()>::new();
    for object in &scene.objects {
        used.insert(object.slot.unwrap_or(0), ());
    }
    if used.len() < 2 {
        return Vec::new();
    }
    let changes = scene
        .gcode
        .as_ref()
        .map(|facts| facts.tool_changes)
        .filter(|count| *count > 0)
        .unwrap_or((used.len() - 1) as u32);
    let saved = changes as f64 * GRAMS_SAVED_PER_CHANGE;
    if saved < MIN_SAVED_G {
        return Vec::new();
    }
    let mut settings = BTreeMap::new();
    settings.insert("flush_into_infill".into(), "1".into());
    settings.insert("flush_into_objects".into(), "1".into());
    vec![tip(
        "purge_saving",
        "purge_saving".into(),
        format!("Flush into infill, about {saved:.1} g"),
        format!(
            "{changes} colour change{} would purge onto the chute. Flushing into the infill keeps about {saved:.1} g of that in the print instead.",
            if changes == 1 { "" } else { "s" }
        ),
        Vec::new(),
        Some(SuggestionFix {
            label: "Flush into infill".into(),
            settings,
            ..SuggestionFix::default()
        }),
        42,
    )]
}

fn setting_plate<'a>(scene: &'a Scene, key: &str) -> Option<&'a str> {
    scene.settings.get(key).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};

    #[test]
    fn one_colour_does_not_mention_purge() {
        let mesh = box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let mut plate = scene(vec![object("a", mesh)]);
        plate.printer.multi_material = true;
        assert!(check(&plate).is_empty());
    }

    #[test]
    fn two_slots_name_the_grams() {
        let mut a = object("a", box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]));
        a.slot = Some(0);
        let mut b = object("b", box_mesh([20.0, 0.0, 0.0], [30.0, 10.0, 10.0]));
        b.slot = Some(1);
        let mut plate = scene(vec![a, b]);
        plate.printer.multi_material = true;
        let found = check(&plate);
        assert_eq!(found.len(), 1);
        assert!(found[0].title.contains('g'), "{}", found[0].title);
    }
}
