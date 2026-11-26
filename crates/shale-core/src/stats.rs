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

#[derive(Clone, Debug, Serialize)]
pub struct SliceStats {
    /// Move length divided by the feed rate on that move. This is not the
    /// CuraEngine header estimate. See [`Self::header_time_seconds`].
    pub print_time_seconds: f64,
    /// CuraEngine's `;TIME:` header, or the last `;TIME_ELAPSED:` when the
    /// header is still the 6666 placeholder.
    pub header_time_seconds: Option<f64>,
    /// Says which field is the feed-rate estimate and which is the header.
    pub time_note: String,
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
        header_time_seconds: crate::gcode::declared_time_seconds(gcode),
        time_note: "print_time_seconds is a feed-rate estimate (distance divided by the F value). header_time_seconds is the CuraEngine ;TIME: header, or the last ;TIME_ELAPSED: when that header is still 6666.".into(),
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
        extruders,
        purge_grams,
        layer_seconds: include_layers.then_some(facts.layer_seconds),
        warnings,
    }
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

    #[test]
    fn header_time_sits_beside_the_feed_rate_estimate() {
        let gcode = "\
;TIME:990
;LAYER:0
;TYPE:WALL-OUTER
G1 F3000 X30 Y0 E1
";
        let stats = from_gcode(gcode, 1.75, 20.0, false, Vec::new());
        assert_eq!(stats.header_time_seconds, Some(990.0));
        assert!(stats.print_time_seconds < 100.0, "{}", stats.print_time_seconds);
        assert!(stats.time_note.contains("feed-rate estimate"));
        assert!(stats.time_note.contains("header_time_seconds"));
    }
}
