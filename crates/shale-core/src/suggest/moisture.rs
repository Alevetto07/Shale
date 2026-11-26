//! A drying reminder, only when the printer has actually measured something.

use super::{tip, Scene, Suggestion};

const HYGROSCOPIC: &[&str] = &["PETG", "PA", "TPU", "PVA"];
/// AMS humidity at or above this is worth a reminder.
pub const HUMIDITY_PERCENT: f64 = 30.0;
/// Hours since the spool was dried, when the spool record says so.
pub const HOURS_SINCE_DRY: f64 = 24.0;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    if scene.humidity_percent.is_none() && scene.hours_since_dry.is_none() {
        return Vec::new();
    }
    let wet = scene.humidity_percent.unwrap_or(0.0) >= HUMIDITY_PERCENT;
    let stale = scene.hours_since_dry.unwrap_or(0.0) >= HOURS_SINCE_DRY;
    if !wet && !stale {
        return Vec::new();
    }
    let mut kinds = Vec::new();
    for slot in scene.slots.iter().filter(|slot| slot.loaded) {
        let kind = super::filament_kind(&slot.filament_type);
        if HYGROSCOPIC.contains(&kind.as_str()) && !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    if kinds.is_empty() {
        return Vec::new();
    }
    let why = match (scene.humidity_percent, scene.hours_since_dry) {
        (Some(humidity), _) if humidity >= HUMIDITY_PERCENT => format!(
            "The AMS reports {humidity:.0}% humidity and {} is loaded. A dry box or a few hours of drying keeps it from stringing.",
            kinds.join(", ")
        ),
        (_, Some(hours)) => format!(
            "The spool data says {} was dried {hours:.0} hours ago. PETG, nylon, TPU and PVA take on water again after that.",
            kinds.join(", ")
        ),
        _ => return Vec::new(),
    };
    vec![tip(
        "dry_filament",
        "dry_filament".into(),
        "Dry the filament",
        why,
        Vec::new(),
        None,
        25,
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::testing::scene;

    #[test]
    fn no_humidity_and_no_spool_data_skips() {
        let mut plate = scene(vec![]);
        plate.slots[0].filament_type = "PETG".into();
        assert!(check(&plate).is_empty());
    }

    #[test]
    fn a_damp_ams_mentions_petg() {
        let mut plate = scene(vec![]);
        plate.slots[0].filament_type = "PETG".into();
        plate.humidity_percent = Some(42.0);
        let found = check(&plate);
        assert_eq!(found.len(), 1);
        assert!(found[0].reason.contains("42%"));
    }

    #[test]
    fn pla_is_not_nagged() {
        let mut plate = scene(vec![]);
        plate.humidity_percent = Some(50.0);
        assert!(check(&plate).is_empty());
    }
}
