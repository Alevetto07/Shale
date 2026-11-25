//! Swap the support interface to the other of PLA and PETG.

use std::collections::BTreeMap;

use super::{is_on, object_slot, setting, tip, Scene, Suggestion, SuggestionFix};

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    if !scene.printer.multi_material {
        return Vec::new();
    }
    let loaded: Vec<_> = scene.slots.iter().filter(|slot| slot.loaded).collect();
    if loaded.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for object in &scene.objects {
        if !is_on(setting(scene, object, "support_enable")) {
            continue;
        }
        let Some(part) = object_slot(scene, object) else {
            continue;
        };
        let part_kind = super::filament_kind(&part.filament_type);
        let other_kind = if part_kind == "PETG" {
            "PLA"
        } else if part_kind == "PLA" {
            "PETG"
        } else {
            continue;
        };
        let Some(other) = loaded.iter().find(|slot| {
            slot.index != part.index && super::filament_kind(&slot.filament_type) == other_kind
        }) else {
            continue;
        };
        let mut settings = BTreeMap::new();
        settings.insert(
            "support_interface_extruder_nr".into(),
            other.index.to_string(),
        );
        settings.insert("support_interface_enable".into(), "true".into());
        out.push(tip(
            "support_interface",
            format!("support_interface:{}", object.id),
            format!("Print the support interface in {other_kind}"),
            format!(
                "The part is {part_kind} and slot {} has {other_kind} loaded. Using {other_kind} only on the interface lets the support snap off. A tool change still purges a little filament, so the saving is a cleaner face, not a cheaper print.",
                other.index + 1
            ),
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: format!("Interface in {other_kind}"),
                object_settings: BTreeMap::from([(object.id.clone(), settings)]),
                ..SuggestionFix::default()
            }),
            65,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::TriangleMesh;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};
    use crate::suggest::FilamentSlot;

    fn cube() -> TriangleMesh {
        box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0])
    }

    fn slots(kinds: &[(&str, bool)]) -> Vec<FilamentSlot> {
        kinds
            .iter()
            .enumerate()
            .map(|(index, (kind, loaded))| FilamentSlot {
                index,
                material_id: kind.to_ascii_lowercase(),
                filament_type: (*kind).to_string(),
                colour: String::new(),
                loaded: *loaded,
            })
            .collect()
    }

    #[test]
    fn petg_with_pla_loaded_offers_the_interface() {
        let mut plate = scene(vec![object("part", cube())]);
        plate.printer.multi_material = true;
        plate.settings.insert("support_enable".into(), "1".into());
        plate.slots = slots(&[("PETG", true), ("PLA", true)]);
        let found = check(&plate);
        assert_eq!(found.len(), 1);
        let settings = &found[0].fix.as_ref().unwrap().object_settings["part"];
        assert_eq!(
            settings
                .get("support_interface_extruder_nr")
                .map(String::as_str),
            Some("1")
        );
        assert!(
            found[0].reason.to_lowercase().contains("purge") || found[0].reason.contains("purges")
        );
    }

    #[test]
    fn pla_with_petg_loaded_uses_the_petg_slot() {
        let mut plate = scene(vec![object("part", cube())]);
        plate.printer.multi_material = true;
        plate
            .settings
            .insert("support_enable".into(), "true".into());
        plate.slots = slots(&[("PLA", true), ("Generic PETG", true)]);
        let found = check(&plate);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].fix.as_ref().unwrap().object_settings["part"]
                .get("support_interface_extruder_nr")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn a_single_material_printer_stays_quiet() {
        let mut plate = scene(vec![object("part", cube())]);
        plate.settings.insert("support_enable".into(), "1".into());
        plate.slots = slots(&[("PETG", true), ("PLA", true)]);
        assert!(check(&plate).is_empty());
    }

    #[test]
    fn missing_pla_does_not_invent_a_slot() {
        let mut plate = scene(vec![object("part", cube())]);
        plate.printer.multi_material = true;
        plate.settings.insert("support_enable".into(), "1".into());
        plate.slots = slots(&[("PETG", true), ("ABS", true)]);
        assert!(check(&plate).is_empty());
    }
}
