//! After a slice, the one thing that is eating the time.

use std::collections::BTreeMap;

use super::{tip, Scene, Suggestion, SuggestionFix};

pub const SUPPORT_FRACTION: f64 = 0.22;
pub const IRONING_FRACTION: f64 = 0.08;
pub const TRAVEL_FRACTION: f64 = 0.30;

pub fn check(scene: &Scene) -> Vec<Suggestion> {
    let Some(facts) = scene.gcode.as_ref() else {
        return Vec::new();
    };
    if facts.seconds < 30.0 {
        return Vec::new();
    }
    let mut sinks = Vec::new();
    if facts.support_seconds / facts.seconds >= SUPPORT_FRACTION {
        let mut settings = BTreeMap::new();
        settings.insert("support_infill_rate".into(), "8".into());
        sinks.push((
            facts.support_seconds,
            "Supports are a large share of the time",
            format!(
                "Supports take about {:.0} minutes of {:.0}. A sparser support infill is the quicker option.",
                facts.support_seconds / 60.0,
                facts.seconds / 60.0
            ),
            "Sparse supports",
            settings,
        ));
    }
    if facts.ironing_seconds / facts.seconds >= IRONING_FRACTION {
        let mut settings = BTreeMap::new();
        settings.insert("ironing_enabled".into(), "false".into());
        sinks.push((
            facts.ironing_seconds,
            "Ironing is a large share of the time",
            format!(
                "Ironing takes about {:.0} minutes. Turning it off is the one-click faster print.",
                facts.ironing_seconds / 60.0
            ),
            "Turn ironing off",
            settings,
        ));
    }
    if facts.travel_seconds / facts.seconds >= TRAVEL_FRACTION {
        let mut settings = BTreeMap::new();
        settings.insert("speed_travel".into(), "500".into());
        sinks.push((
            facts.travel_seconds,
            "Travel is a large share of the time",
            format!(
                "Travel takes about {:.0} minutes of {:.0}. A faster travel speed is the direct cut.",
                facts.travel_seconds / 60.0,
                facts.seconds / 60.0
            ),
            "Faster travel",
            settings,
        ));
    }
    sinks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let Some((_, title, reason, label, settings)) = sinks.into_iter().next() else {
        return Vec::new();
    };
    vec![tip(
        "time_sink",
        "time_sink".into(),
        title,
        reason,
        Vec::new(),
        Some(SuggestionFix {
            label: label.into(),
            settings,
            ..SuggestionFix::default()
        }),
        58,
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suggest::gcode::facts;
    use crate::suggest::testing::scene;

    #[test]
    fn without_gcode_there_is_nothing_to_time() {
        assert!(check(&scene(vec![])).is_empty());
    }

    #[test]
    fn ironing_that_dominates_offers_to_turn_it_off() {
        let gcode = "\
;TYPE:IRONING
G1 X600 Y0 E1 F600
G1 X0 Y0 E2 F600
;TYPE:WALL-OUTER
G1 X10 Y0 E2.1 F3000
";
        let mut plate = scene(vec![]);
        plate.gcode = Some(facts(gcode));
        let found = check(&plate);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0]
                .fix
                .as_ref()
                .unwrap()
                .settings
                .get("ironing_enabled")
                .map(String::as_str),
            Some("false")
        );
    }
}
