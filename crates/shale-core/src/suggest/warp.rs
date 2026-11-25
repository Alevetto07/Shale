//! Large flat footprints in PETG, ABS or ASA.

use std::collections::BTreeMap;

use super::geom::contact;
use super::{object_slot, placed, tip, Scene, Suggestion, SuggestionFix};

/// Footprint at or above this, in mm², is a large flat first layer.
pub const LARGE_FOOTPRINT_MM2: f64 = 8_000.0;
/// One side this long, with at least [`LONG_SIDE_AREA_MM2`] of contact.
pub const LONG_SIDE_MM: f64 = 140.0;
pub const LONG_SIDE_AREA_MM2: f64 = 4_000.0;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for object in &scene.objects {
        let Some(slot) = object_slot(scene, object) else {
            continue;
        };
        let kind = super::filament_kind(&slot.filament_type);
        if kind != "PETG" && kind != "ABS" && kind != "ASA" {
            continue;
        }
        let shape = contact(&placed(object));
        let long_side = shape.base_x_mm.max(shape.base_y_mm) >= LONG_SIDE_MM
            && shape.area_mm2 >= LONG_SIDE_AREA_MM2;
        if shape.area_mm2 < LARGE_FOOTPRINT_MM2 && !long_side {
            continue;
        }
        let open = !scene.printer.enclosed;
        let mut settings = BTreeMap::new();
        settings.insert("adhesion_type".into(), "brim".into());
        settings.insert("brim_width".into(), "6".into());
        let (title, reason, warning) = if (kind == "ABS" || kind == "ASA") && open {
            settings.insert("draft_shield".into(), "limited".into());
            (
                format!("{kind} on an open printer"),
                format!(
                    "The first layer is about {:.0} mm² of {kind}. An open printer (this one is) lets it cool and lift. A brim and a draft shield hold the edges; an enclosure is the real fix.",
                    shape.area_mm2
                ),
                true,
            )
        } else if kind == "PETG" {
            let bed = scene.printer.bed_temperature_c;
            if bed < 70.0 {
                settings.insert("material_bed_temperature".into(), "70".into());
            }
            (
                "A large PETG footprint".to_string(),
                format!(
                    "About {:.0} mm² of PETG on the first layer can lift at the corners. A brim helps, and the bed wants to be at least 70 °C. {}.",
                    shape.area_mm2,
                    if scene.printer.enclosed {
                        "The printer is enclosed, which already keeps the draught off"
                    } else {
                        "This printer is open, so keep it out of a draught"
                    }
                ),
                false,
            )
        } else {
            (
                format!("{kind} on a large base"),
                format!(
                    "The first layer is about {:.0} mm². The enclosure helps, and a brim still holds the corners.",
                    shape.area_mm2
                ),
                false,
            )
        };
        let mut item = tip(
            "warp_risk",
            format!("warp_risk:{}", object.id),
            title,
            reason,
            vec![object.id.clone()],
            Some(SuggestionFix {
                label: "Add a brim".into(),
                settings,
                ..SuggestionFix::default()
            }),
            if warning { 75 } else { 45 },
        );
        if warning {
            item.severity = super::Severity::Warning;
        }
        out.push(item);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::geom::box_mesh;
    use crate::suggest::testing::{object, scene};

    #[test]
    fn a_small_petg_cube_is_quiet() {
        let mut plate = scene(vec![object(
            "cube",
            box_mesh([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]),
        )]);
        plate.slots[0].filament_type = "PETG".into();
        assert!(check(&plate).is_empty());
    }

    #[test]
    fn a_large_abs_plate_on_an_a1_warns() {
        let mut plate = scene(vec![object(
            "panel",
            box_mesh([0.0, 0.0, 0.0], [150.0, 120.0, 3.0]),
        )]);
        plate.slots[0].filament_type = "ABS".into();
        let found = check(&plate);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].severity, crate::suggest::Severity::Warning);
        assert_eq!(
            found[0]
                .fix
                .as_ref()
                .unwrap()
                .settings
                .get("draft_shield")
                .map(String::as_str),
            Some("limited")
        );
    }

    #[test]
    fn an_enclosed_printer_does_not_ask_for_a_shield_on_abs() {
        let mut plate = scene(vec![object(
            "panel",
            box_mesh([0.0, 0.0, 0.0], [150.0, 120.0, 3.0]),
        )]);
        plate.printer.id = "bambu_lab_x1_carbon".into();
        plate.printer.enclosed = true;
        plate.slots[0].filament_type = "ABS".into();
        let found = check(&plate);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].severity, crate::suggest::Severity::Tip);
        assert!(!found[0]
            .fix
            .as_ref()
            .unwrap()
            .settings
            .contains_key("draft_shield"));
    }
}
