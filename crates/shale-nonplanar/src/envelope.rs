//! Toolhead envelope models for swept collision checks.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Debug, Deserialize)]
pub struct HeadEnvelope {
    pub printer_id: String,
    pub calibrated: bool,
    #[serde(default)]
    pub calibration_tier: Option<String>,
    #[serde(default)]
    pub needs_measurement: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
    pub nozzle_height_mm: f64,
    pub tip_land_radius_mm: f64,
    pub nozzle_cone_half_angle_deg: f64,
    pub body_radius_mm: f64,
    pub body_top_z_mm: f64,
    pub sock_outer_radius_mm: f64,
    pub sock_top_z_mm: f64,
    pub duct_x_offset_mm: [f64; 2],
    pub duct_z_mm: f64,
    pub default_max_clearance_angle_deg: f64,
}

impl HeadEnvelope {
    /// Lowest point of the toolhead envelope relative to the nozzle tip (Z=0 at tip).
    pub fn lowest_extent_z(&self) -> f64 {
        -self.tip_land_radius_mm
    }

    /// Horizontal reach of the envelope at a given height above the nozzle tip.
    pub fn radius_at_z(&self, z_above_tip: f64) -> f64 {
        let mut r = self.tip_land_radius_mm;
        if z_above_tip <= 0.0 {
            return r;
        }
        let cone = self.nozzle_cone_half_angle_deg.to_radians().tan() * z_above_tip;
        r = r.max(cone);
        if z_above_tip <= self.body_top_z_mm {
            r = r.max(self.body_radius_mm);
        }
        if z_above_tip <= self.sock_top_z_mm {
            r = r.max(self.sock_outer_radius_mm);
        }
        if z_above_tip <= self.duct_z_mm {
            r = r.max(self.duct_x_offset_mm[1]);
        }
        r
    }
}

static ENVELOPES: OnceLock<HashMap<String, HeadEnvelope>> = OnceLock::new();

fn load_envelopes() -> &'static HashMap<String, HeadEnvelope> {
    ENVELOPES.get_or_init(|| {
        let files: &[(&str, &str)] = &[
            ("bambu_lab_a1", include_str!("../data/bambu_a1_head.json")),
            (
                "bambu_lab_a1_mini",
                include_str!("../data/bambu_a1_mini_head.json"),
            ),
            ("bambu_lab_p1s", include_str!("../data/bambu_p1s_head.json")),
            ("bambu_lab_p1p", include_str!("../data/bambu_p1p_head.json")),
            (
                "bambu_lab_x1_carbon",
                include_str!("../data/bambu_x1_carbon_head.json"),
            ),
            ("bambu_lab_h2d", include_str!("../data/bambu_h2d_head.json")),
        ];
        let mut map = HashMap::new();
        for (id, text) in files {
            let env: HeadEnvelope =
                serde_json::from_str(text).unwrap_or_else(|err| panic!("{id} envelope JSON: {err}"));
            map.insert(id.to_string(), env);
        }
        map
    })
}

/// Catalogue printer ids that ship a head envelope.
pub fn envelope_catalogue() -> Vec<&'static str> {
    vec![
        "bambu_lab_a1",
        "bambu_lab_a1_mini",
        "bambu_lab_p1s",
        "bambu_lab_p1p",
        "bambu_lab_x1_carbon",
        "bambu_lab_h2d",
    ]
}

pub fn head_envelope_for_printer(printer_id: &str) -> Option<&'static HeadEnvelope> {
    let key = printer_id.to_ascii_lowercase();
    let map = load_envelopes();
    map.get(&key).or_else(|| {
        // Accept common aliases (e.g. UI shorthand).
        if key == "bambu_lab_x1c" {
            return map.get("bambu_lab_x1_carbon");
        }
        None
    })
}

pub fn bambu_a1_envelope() -> &'static HeadEnvelope {
    head_envelope_for_printer("bambu_lab_a1").expect("embedded A1 envelope")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_a1_envelope_loads() {
        let env = bambu_a1_envelope();
        assert!(env.calibrated);
        assert!((env.nozzle_height_mm - 4.76).abs() < 0.01);
        assert!(env.radius_at_z(5.0) > env.tip_land_radius_mm);
        assert!(!env.needs_measurement.is_empty());
    }

    #[test]
    fn all_catalogue_envelopes_load() {
        for id in envelope_catalogue() {
            let env = head_envelope_for_printer(id).expect(id);
            assert!(env.calibrated);
            assert_eq!(env.printer_id, id);
        }
    }
}