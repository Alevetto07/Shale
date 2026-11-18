//! What a print costs: filament, energy, and the arithmetic that joins them.
//!
//! The numbers here are all "given the engine said so": filament length comes
//! from the slice, print time from the slice, and everything else — density,
//! price, wattage, tariff — is a setting the user can see and change. Nothing
//! in this module guesses at a figure it was not handed, so a job card that
//! shows a total is showing arithmetic over the file on screen.

use serde::{Deserialize, Serialize};

/// A price no one has to defend. Twenty euro a kilo is roughly what a spool of
/// PLA costs, and the field is editable: this is a starting point, not a claim.
pub const DEFAULT_PRICE_PER_KG: f64 = 20.0;
/// An A1 on a normal print sits near 100 W averaged over the job; the heater is
/// the bulk of it and the motors are small.
pub const DEFAULT_WATTS: f64 = 100.0;
pub const DEFAULT_ELECTRICITY_PER_KWH: f64 = 0.30;
pub const DEFAULT_CURRENCY: &str = "EUR";

/// The editable half of a cost estimate, kept in the app's settings file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostSettings {
    /// Shown next to the number. Free text, because "EUR" is not the only
    /// answer and a symbol the fonts lack would render as a box.
    pub currency: String,
    pub price_per_kg: f64,
    /// Average draw while printing, in watts. Not the nameplate: an A1's bed
    /// and hotend do not run flat out for the whole job.
    pub watts: f64,
    pub electricity_per_kwh: f64,
}

impl Default for CostSettings {
    fn default() -> Self {
        Self {
            currency: DEFAULT_CURRENCY.to_string(),
            price_per_kg: DEFAULT_PRICE_PER_KG,
            watts: DEFAULT_WATTS,
            electricity_per_kwh: DEFAULT_ELECTRICITY_PER_KWH,
        }
    }
}

/// One spool's contribution: how much of it the job uses, and what that costs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SpoolCost {
    /// The AMS slot, or 0 for the plate's own material.
    pub slot: usize,
    pub metres: f64,
    pub grams: f64,
    pub cost: f64,
}

/// The whole estimate, as the job card shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CostEstimate {
    pub spools: Vec<SpoolCost>,
    pub grams: f64,
    pub filament_cost: f64,
    pub kwh: f64,
    pub energy_cost: f64,
    pub total: f64,
    pub currency: String,
}

/// Grams of filament in `metres` of a round strand.
///
/// The strand is a cylinder: `π r²` of cross-section times the length, in
/// cubic millimetres, then a thousandth of that in cubic centimetres, times the
/// density in g/cm³. A 1.75 mm strand of PLA at 1.24 g/cm³ is 2.98 g a metre,
/// which is the number every slicer agrees on.
pub fn filament_grams(metres: f64, diameter_mm: f64, density_g_cm3: f64) -> f64 {
    let radius = diameter_mm / 2.0;
    let area_mm2 = std::f64::consts::PI * radius * radius;
    let volume_cm3 = area_mm2 * (metres * 1000.0) / 1000.0;
    volume_cm3 * density_g_cm3
}

/// What `metres` of filament costs at `price_per_kg`.
pub fn filament_cost(metres: f64, diameter_mm: f64, density_g_cm3: f64, price_per_kg: f64) -> f64 {
    filament_grams(metres, diameter_mm, density_g_cm3) / 1000.0 * price_per_kg
}

/// Kilowatt-hours drawn by `watts` over `seconds`.
pub fn energy_kwh(watts: f64, seconds: f64) -> f64 {
    watts * seconds / 3600.0 / 1000.0
}

/// The estimate for a plate.
///
/// `spools` is one entry per spool that contributes filament, in the order the
/// UI should print them. A single-material plate passes one entry; an AMS plate
/// passes one per slot that the slice actually used. Empty `spools` still
/// yields the energy half, because time is known even when length is not.
#[allow(clippy::too_many_arguments)]
pub fn estimate(
    spools: &[(usize, f64)],
    diameter_mm: f64,
    density_g_cm3: f64,
    print_seconds: Option<f64>,
    settings: &CostSettings,
) -> CostEstimate {
    let lines: Vec<SpoolCost> = spools
        .iter()
        .map(|(slot, metres)| SpoolCost {
            slot: *slot,
            metres: *metres,
            grams: filament_grams(*metres, diameter_mm, density_g_cm3),
            cost: filament_cost(*metres, diameter_mm, density_g_cm3, settings.price_per_kg),
        })
        .collect();
    let grams: f64 = lines.iter().map(|line| line.grams).sum();
    let filament_cost: f64 = lines.iter().map(|line| line.cost).sum();
    let kwh = energy_kwh(settings.watts, print_seconds.unwrap_or(0.0));
    let energy_cost = kwh * settings.electricity_per_kwh;
    CostEstimate {
        spools: lines,
        grams,
        filament_cost,
        kwh,
        energy_cost,
        total: filament_cost + energy_cost,
        currency: settings.currency.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PLA at 1.24 g/cm³ through a 1.75 mm nozzle: 2.98 g a metre, the figure
    /// Cura and PrusaSlicer both report for the same strand.
    #[test]
    fn a_metre_of_pla_weighs_what_every_slicer_says() {
        let grams = filament_grams(1.0, 1.75, 1.24);
        assert!((grams - 2.982).abs() < 0.01, "got {grams}");
        // Twice the length, twice the weight: the formula is linear.
        assert!((filament_grams(2.0, 1.75, 1.24) - grams * 2.0).abs() < 1e-9);
        // And nothing weighs nothing.
        assert_eq!(filament_grams(0.0, 1.75, 1.24), 0.0);
    }

    #[test]
    fn a_kilo_of_filament_costs_what_the_spool_cost() {
        // 1 kg at 20 EUR/kg is 20 EUR, however it is cut.
        let metres = 1000.0 / filament_grams(1.0, 1.75, 1.24);
        let cost = filament_cost(metres, 1.75, 1.24, 20.0);
        assert!((cost - 20.0).abs() < 1e-6, "got {cost}");
    }

    #[test]
    fn energy_is_watts_times_hours() {
        // 100 W for an hour is a tenth of a unit of electricity.
        assert!((energy_kwh(100.0, 3600.0) - 0.1).abs() < 1e-9);
        assert_eq!(energy_kwh(100.0, 0.0), 0.0);
        // Half an hour is half as much.
        assert!((energy_kwh(100.0, 1800.0) - 0.05).abs() < 1e-9);
    }

    #[test]
    fn two_spools_are_two_lines_that_sum_to_the_total() {
        let settings = CostSettings::default();
        let estimate = estimate(&[(0, 10.0), (1, 5.0)], 1.75, 1.24, Some(3600.0), &settings);
        assert_eq!(estimate.spools.len(), 2, "one line per spool");
        assert_eq!(estimate.spools[0].slot, 0);
        assert_eq!(estimate.spools[1].slot, 1);
        // The second line is half the first, because it is half the length.
        assert!(
            (estimate.spools[0].grams - estimate.spools[1].grams * 2.0).abs() < 1e-9,
            "{:?}",
            estimate.spools
        );
        let summed: f64 = estimate.spools.iter().map(|line| line.cost).sum();
        assert!((summed - estimate.filament_cost).abs() < 1e-9);
        assert!((estimate.total - estimate.filament_cost - estimate.energy_cost).abs() < 1e-9);
        // 15 m of PLA at 20 EUR/kg plus 0.1 kWh at 0.30: about 0.93 EUR.
        assert!(
            (estimate.total - (15.0 * 2.982 / 1000.0 * 20.0 + 0.03)).abs() < 0.01,
            "got {}",
            estimate.total
        );
        assert_eq!(estimate.currency, "EUR");
    }

    #[test]
    fn an_ams_plate_keeps_its_slots_apart() {
        let settings = CostSettings {
            price_per_kg: 30.0,
            ..CostSettings::default()
        };
        let estimate = estimate(
            &[(0, 1.0), (2, 2.0), (3, 4.0)],
            1.75,
            1.24,
            Some(600.0),
            &settings,
        );
        let slots: Vec<usize> = estimate.spools.iter().map(|line| line.slot).collect();
        assert_eq!(slots, vec![0, 2, 3]);
        // Slot 3 uses four times slot 0's length, so four times its cost.
        let ratio = estimate.spools[2].cost / estimate.spools[0].cost;
        assert!((ratio - 4.0).abs() < 1e-6, "got {ratio}");
    }

    #[test]
    fn an_estimate_without_a_time_still_reports_filament() {
        let estimate = estimate(&[(0, 2.0)], 1.75, 1.24, None, &CostSettings::default());
        assert!(estimate.filament_cost > 0.0);
        assert_eq!(estimate.kwh, 0.0);
        assert_eq!(estimate.energy_cost, 0.0);
        assert!((estimate.total - estimate.filament_cost).abs() < 1e-12);
    }
}
