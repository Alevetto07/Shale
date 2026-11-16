//! Toolhead envelope models for swept collision checks.

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Clone, Debug, Deserialize)]
pub struct HeadEnvelope {
    pub printer_id: String,
    pub calibrated: bool,
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

static A1: OnceLock<HeadEnvelope> = OnceLock::new();

pub fn bambu_a1_envelope() -> &'static HeadEnvelope {
    A1.get_or_init(|| {
        let text = include_str!("../data/bambu_a1_head.json");
        serde_json::from_str(text).expect("embedded A1 head envelope JSON")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_a1_envelope_loads() {
        let env = bambu_a1_envelope();
        assert!(!env.calibrated);
        assert!((env.nozzle_height_mm - 4.76).abs() < 0.01);
        assert!(env.radius_at_z(5.0) > env.tip_land_radius_mm);
    }
}