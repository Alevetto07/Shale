//! The project filament does not match what the printer has loaded.

use std::collections::BTreeMap;

use super::{Scene, Suggestion, SuggestionFix};

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let Some(device) = scene.device_slots.as_ref() else {
        return Vec::new();
    };
    let mut mismatches = Vec::new();
    for slot in scene.slots.iter().filter(|slot| slot.loaded) {
        let Some(loaded) = device
            .iter()
            .find(|item| item.index == slot.index && item.loaded)
        else {
            mismatches.push(format!(
                "slot {} is {} in the project and empty on the printer",
                slot.index + 1,
                super::filament_kind(&slot.filament_type)
            ));
            continue;
        };
        let wanted = super::filament_kind(&slot.filament_type);
        let found = super::filament_kind(&loaded.filament_type);
        if wanted != "UNKNOWN" && found != "UNKNOWN" && wanted != found {
            mismatches.push(format!(
                "slot {} is {wanted} in the project and {found} on the printer",
                slot.index + 1
            ));
        }
    }
    if mismatches.is_empty() {
        return Vec::new();
    }
    let mut slots = BTreeMap::new();
    for object in &scene.objects {
        let index = object.slot.unwrap_or(0);
        let Some(project) = scene.slots.iter().find(|slot| slot.index == index) else {
            continue;
        };
        let wanted = super::filament_kind(&project.filament_type);
        let same = device
            .iter()
            .find(|slot| slot.loaded && super::filament_kind(&slot.filament_type) == wanted);
        if let Some(target) = same {
            if target.index != index {
                slots.insert(object.id.clone(), target.index);
            }
        }
    }
    let mut item = super::tip(
        "ams_mismatch",
        "ams_mismatch".into(),
        "The AMS does not match the project",
        format!(
            "{}. {}",
            mismatches.join("; "),
            if slots.is_empty() {
                "Load the project's filament, or dismiss this if the printer is right.".to_string()
            } else {
                "The parts can move to the slots that already hold the same plastic.".to_string()
            }
        ),
        slots.keys().cloned().collect(),
        (!slots.is_empty()).then(|| SuggestionFix {
            label: "Map to the loaded slots".into(),
            slots,
            ..SuggestionFix::default()
        }),
        90,
    );
    item.severity = super::Severity::Warning;
    vec![item]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};
    use crate::suggest::FilamentSlot;

    fn slot(index: usize, kind: &str) -> FilamentSlot {
        FilamentSlot {
            index,
            material_id: kind.into(),
            filament_type: kind.into(),
            colour: String::new(),
            loaded: true,
        }
    }

    #[test]
    fn no_printer_means_no_warning() {
        let plate = scene(vec![object(
            "a",
            box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]),
        )]);
        assert!(check(&plate).is_empty());
    }

    #[test]
    fn a_swapped_pair_offers_the_mapping() {
        let mut part = object("a", box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]));
        part.slot = Some(0);
        let mut plate = scene(vec![part]);
        plate.slots = vec![slot(0, "PETG"), slot(1, "PLA")];
        plate.device_slots = Some(vec![slot(0, "PLA"), slot(1, "PETG")]);
        let found = check(&plate);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, crate::suggest::Severity::Warning);
        assert_eq!(
            found[0].fix.as_ref().unwrap().slots.get("a").copied(),
            Some(1)
        );
    }
}
