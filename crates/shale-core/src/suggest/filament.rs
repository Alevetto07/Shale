//! A filament that does not match an explicit job, very rarely.

use super::{is_on, number, object_slot, tip, Scene, Suggestion};

/// TPU above this is a high-speed profile, not a flexible one.
pub const TPU_SPEED_MM_S: f64 = 100.0;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    if scene.shown_once.iter().any(|id| id == "filament_fit") {
        return Vec::new();
    }
    let speed = number(scene.settings.get("speed_print").map(String::as_str))
        .unwrap_or(scene.printer.speed_mm_s);
    let hot = scene.tags.iter().any(|tag| {
        let tag = tag.to_ascii_lowercase();
        tag == "hot" || tag == "hot-use"
    }) || is_on(scene.settings.get("shale_hot_use").map(String::as_str));
    for object in &scene.objects {
        let Some(slot) = object_slot(scene, object) else {
            continue;
        };
        let kind = super::filament_kind(&slot.filament_type);
        // The name and the shape are not evidence. A desk model is still a desk model.
        if kind == "TPU" && speed > TPU_SPEED_MM_S {
            let mut item = tip(
                "filament_fit",
                "filament_fit".into(),
                "TPU on a fast profile",
                format!(
                    "TPU is loaded and the print speed is {speed:.0} mm/s. Flexible filament usually wants a much slower profile. This is a tip, and dismissing it keeps it away."
                ),
                vec![object.id.clone()],
                None,
                20,
            );
            item.once = true;
            return vec![item];
        }
        if hot && kind == "PLA" {
            let mut item = tip(
                "filament_fit",
                "filament_fit".into(),
                "PLA on a hot-use part",
                "This project is tagged as hot-use and the filament is PLA. PLA softens on a warm part. Nothing here is guessed from the shape or the file name.",
                vec![object.id.clone()],
                None,
                20,
            );
            item.once = true;
            return vec![item];
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};
    use crate::suggest::FilamentSlot;

    #[test]
    fn a_rocket_shaped_name_does_not_warn_about_pla() {
        let mut part = object(
            "desk-rocket-engine",
            box_mesh([0.0, 0.0, 0.0], [30.0, 30.0, 80.0]),
        );
        part.name = "rocket engine".into();
        let mut plate = scene(vec![part]);
        plate.slots[0].filament_type = "PLA".into();
        assert!(check(&plate).is_empty());
    }

    #[test]
    fn fast_tpu_is_a_single_tip() {
        let mut plate = scene(vec![object(
            "band",
            box_mesh([0.0, 0.0, 0.0], [20.0, 10.0, 5.0]),
        )]);
        plate.slots = vec![FilamentSlot {
            index: 0,
            material_id: "tpu".into(),
            filament_type: "TPU".into(),
            colour: String::new(),
            loaded: true,
        }];
        plate.printer.speed_mm_s = 200.0;
        let found = check(&plate);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, crate::suggest::Severity::Tip);
        assert!(found[0].once);
        plate.shown_once.push("filament_fit".into());
        assert!(check(&plate).is_empty());
    }

    #[test]
    fn slow_tpu_is_left_alone() {
        let mut plate = scene(vec![object(
            "band",
            box_mesh([0.0, 0.0, 0.0], [20.0, 10.0, 5.0]),
        )]);
        plate.slots[0].filament_type = "TPU".into();
        plate.settings.insert("speed_print".into(), "40".into());
        assert!(check(&plate).is_empty());
    }

    #[test]
    fn an_explicit_hot_tag_mentions_pla() {
        let mut plate = scene(vec![object(
            "clip",
            box_mesh([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]),
        )]);
        plate.tags.push("hot-use".into());
        let found = check(&plate);
        assert_eq!(found.len(), 1);
        assert!(found[0].reason.contains("tagged"));
    }
}
