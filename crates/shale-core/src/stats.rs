//! Numbers a finished slice can answer without another engine run.
//!
//! Time by feature, filament by extruder, purge, and cost all come from the
//! G-code text. Price is the caller's; when they do not give one the default
//! is the same twenty euro a kilo the job card starts from.

use serde::Serialize;

use crate::cost::{self, DEFAULT_PRICE_PER_KG};
use crate::suggest::{gcode_facts, GcodeFacts};

const DEFAULT_DIAMETER_MM: f64 = 1.75;
const DEFAULT_DENSITY: f64 = 1.24;

#[derive(Clone, Debug, Serialize)]
pub struct ExtruderUse {
    pub index: usize,
    pub metres: f64,
    pub grams: f64,
}

/// Time and filament for one preview kind. `metres` is filament length, not travel.
#[derive(Clone, Debug, Serialize)]
pub struct FeatureUse {
    pub kind: String,
    pub seconds: f64,
    pub metres: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SliceStats {
    pub print_time_seconds: f64,
    pub filament_metres: f64,
    pub grams: f64,
    pub cost: f64,
    pub currency: String,
    pub layers: u32,
    pub max_speed_mm_s: f64,
    pub average_speed_mm_s: f64,
    pub walls_seconds: f64,
    pub infill_seconds: f64,
    pub support_seconds: f64,
    pub travel_seconds: f64,
    pub ironing_seconds: f64,
    pub skirt_seconds: f64,
    pub other_seconds: f64,
    /// One row per feature the legend can name, longest first is not required:
    /// the order is the preview's own (walls, then skin, infill, support, the rest).
    pub features: Vec<FeatureUse>,
    pub extruders: Vec<ExtruderUse>,
    pub purge_grams: f64,
    /// One entry per `;LAYER:` when the caller asked for them.
    pub layer_seconds: Option<Vec<f64>>,
    pub warnings: Vec<String>,
}

pub fn from_gcode(
    gcode: &str,
    diameter_mm: f64,
    price_per_kg: f64,
    include_layers: bool,
    warnings: Vec<String>,
) -> SliceStats {
    let facts = gcode_facts(gcode);
    let diameter = if diameter_mm > 0.0 {
        diameter_mm
    } else {
        DEFAULT_DIAMETER_MM
    };
    let price = if price_per_kg > 0.0 {
        price_per_kg
    } else {
        DEFAULT_PRICE_PER_KG
    };
    let metres = if facts.extrude_mm > 0.0 {
        facts.extrude_mm / 1000.0
    } else {
        0.0
    };
    let grams = cost::filament_grams(metres, diameter, DEFAULT_DENSITY);
    let cost = cost::filament_cost(metres, diameter, DEFAULT_DENSITY, price);
    let extruders = extruders_of(&facts, diameter);
    let purge_grams = cost::filament_grams(facts.purge_mm / 1000.0, diameter, DEFAULT_DENSITY);
    SliceStats {
        print_time_seconds: facts.seconds,
        filament_metres: metres,
        grams,
        cost,
        currency: cost::DEFAULT_CURRENCY.to_string(),
        layers: facts.layer_count,
        max_speed_mm_s: facts.max_speed_mm_s,
        average_speed_mm_s: facts.average_speed_mm_s,
        walls_seconds: facts.wall_seconds,
        infill_seconds: facts.infill_seconds,
        support_seconds: facts.support_seconds,
        travel_seconds: facts.travel_seconds,
        ironing_seconds: facts.ironing_seconds,
        skirt_seconds: facts.skirt_seconds,
        other_seconds: facts.other_seconds,
        features: features_of(&facts),
        extruders,
        purge_grams,
        layer_seconds: include_layers.then_some(facts.layer_seconds),
        warnings,
    }
}

const FEATURE_ORDER: &[&str] = &[
    "wall-outer",
    "wall-inner",
    "skin",
    "infill",
    "support",
    "support-interface",
    "brim",
    "raft",
    "skirt",
    "prime-tower",
    "ironing",
    "travel",
];

pub fn features_from_gcode(gcode: &str) -> Vec<FeatureUse> {
    features_of(&gcode_facts(gcode))
}

fn features_of(facts: &GcodeFacts) -> Vec<FeatureUse> {
    let mut kinds: Vec<String> = facts
        .feature_seconds
        .keys()
        .cloned()
        .chain(facts.feature_mm.keys().cloned())
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds.sort_by(|left, right| {
        let left_at = FEATURE_ORDER
            .iter()
            .position(|kind| kind == left)
            .unwrap_or(FEATURE_ORDER.len());
        let right_at = FEATURE_ORDER
            .iter()
            .position(|kind| kind == right)
            .unwrap_or(FEATURE_ORDER.len());
        left_at.cmp(&right_at).then_with(|| left.cmp(right))
    });
    kinds
        .into_iter()
        .map(|kind| FeatureUse {
            seconds: facts.feature_seconds.get(&kind).copied().unwrap_or(0.0),
            metres: facts.feature_mm.get(&kind).copied().unwrap_or(0.0) / 1000.0,
            kind,
        })
        .filter(|item| item.seconds > 1e-6 || item.metres > 1e-9)
        .collect()
}

fn extruders_of(facts: &GcodeFacts, diameter: f64) -> Vec<ExtruderUse> {
    facts
        .per_tool_mm
        .iter()
        .enumerate()
        .filter(|(_, mm)| **mm > 1e-6)
        .map(|(index, mm)| {
            let metres = mm / 1000.0;
            ExtruderUse {
                index,
                metres,
                grams: cost::filament_grams(metres, diameter, DEFAULT_DENSITY),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_two_tool_slice_splits_filament_and_counts_the_purge() {
        let gcode = "\
;LAYER:0
;TYPE:WALL-OUTER
G1 F1200 X10 Y0 E1
;TYPE:PRIME
T1
G1 X20 Y0 E2
;LAYER:1
;TYPE:FILL
G1 X30 Y0 E4
";
        let stats = from_gcode(gcode, 1.75, 20.0, true, Vec::new());
        assert_eq!(stats.layers, 2);
        assert_eq!(stats.extruders.len(), 2);
        assert!(stats.extruders[1].metres > stats.extruders[0].metres);
        assert!(stats.purge_grams > 0.0);
        assert_eq!(stats.layer_seconds.as_ref().map(Vec::len), Some(2));
        assert!(stats.cost > 0.0);
        assert!(stats.walls_seconds > 0.0);
        assert!(stats.infill_seconds > 0.0);
        assert!(stats
            .features
            .iter()
            .any(|item| item.kind == "wall-outer" && item.metres > 0.0));
        assert!(stats
            .features
            .iter()
            .any(|item| item.kind == "infill" && item.seconds > 0.0));
    }

    #[test]
    fn a_bambu_header_does_not_invent_a_thousand_extruders() {
        let gcode = "\
T0
;TYPE:SKIRT
G1 F1200 X10 Y0 E1
T1000
G0 F0 Z0.4
T255
;TYPE:WALL-OUTER
G1 X20 Y0 E2
";
        let stats = from_gcode(gcode, 1.75, 20.0, false, Vec::new());
        assert_eq!(stats.extruders.len(), 1);
        assert!(stats.skirt_seconds > 0.0);
        assert!(stats.travel_seconds < 1.0, "{}", stats.travel_seconds);
        let sum = stats.walls_seconds
            + stats.infill_seconds
            + stats.support_seconds
            + stats.travel_seconds
            + stats.ironing_seconds
            + stats.skirt_seconds
            + stats.other_seconds;
        assert!((sum - stats.print_time_seconds).abs() < 1e-6);
    }
}
