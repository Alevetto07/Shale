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
    ///
    /// The profile is stepped rather than one cylinder: the nozzle cone widens
    /// from the tip land, the carriage body and sock only reach out *above* the
    /// nozzle barrel, and the part-cooling duct hangs above that. Modelling the
    /// body from `h = 0` (as the previous version did) reported phantom
    /// collisions near the tip while ignoring the duct entirely.
    pub fn radius_at_z(&self, z_above_tip: f64) -> f64 {
        if z_above_tip <= 0.0 {
            return self.tip_land_radius_mm;
        }
        let mut r = self.tip_land_radius_mm
            + self.nozzle_cone_half_angle_deg.to_radians().tan() * z_above_tip;
        if z_above_tip > self.nozzle_height_mm {
            r = r.max(self.body_radius_mm).max(self.sock_outer_radius_mm);
        }
        if z_above_tip > self.duct_z_mm {
            // The duct sits to one side of the head; treating it as all-around
            // is conservative and costs only reach, never safety.
            r = r.max(self.duct_x_offset_mm[1]);
        }
        r
    }

    /// Largest horizontal reach anywhere in the profile, used to bound the
    /// collision queries against the printed-height map.
    pub fn max_reach_radius(&self) -> f64 {
        self.tip_land_radius_mm
            .max(self.body_radius_mm)
            .max(self.sock_outer_radius_mm)
            .max(self.duct_x_offset_mm[1])
    }

    /// Smallest height above the tip at which the envelope reaches `radius`.
    pub fn min_height_for_radius(&self, radius: f64) -> f64 {
        if radius <= self.tip_land_radius_mm {
            return 0.0;
        }
        let tan = self.nozzle_cone_half_angle_deg.to_radians().tan();
        let mut best = f64::INFINITY;
        // The cone only reaches this far while it is still inside the barrel.
        let cone = (radius - self.tip_land_radius_mm) / tan;
        if cone > 0.0 && cone <= self.nozzle_height_mm + 1e-9 {
            best = best.min(cone);
        }
        // Above the barrel the body and sock are in the way all around.
        if radius <= self.body_radius_mm.max(self.sock_outer_radius_mm) {
            best = best.min(self.nozzle_height_mm);
        }
        // Above the duct height the duct's all-around reach applies.
        if radius <= self.duct_x_offset_mm[1] {
            best = best.min(self.duct_z_mm);
        }
        best
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

/// Error text used when a job asks for non-planar on a printer without an
/// envelope. Callers must surface this instead of silently slicing planar.
pub fn missing_envelope_message(printer_id: &str) -> String {
    format!(
        "non-planar needs a head envelope; printer {printer_id} has none (supported: {})",
        envelope_catalogue().join(", ")
    )
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

    #[test]
    fn profile_is_stepped_not_a_single_cylinder() {
        let env = bambu_a1_envelope();
        // At the tip only the land is in the way.
        assert!((env.radius_at_z(0.0) - env.tip_land_radius_mm).abs() < 1e-9);
        // A millimetre up, only the cone has widened.
        let cone =
            env.tip_land_radius_mm + env.nozzle_cone_half_angle_deg.to_radians().tan() * 1.0;
        assert!((env.radius_at_z(1.0) - cone).abs() < 1e-9);
        assert!(env.radius_at_z(1.0) < env.body_radius_mm);
        // Just under the barrel height the body is still clear.
        assert!(env.radius_at_z(env.nozzle_height_mm - 0.01) < env.body_radius_mm);
        // Above it the body and sock are in the way.
        let body = env.body_radius_mm.max(env.sock_outer_radius_mm);
        assert!((env.radius_at_z(env.nozzle_height_mm + 0.01) - body).abs() < 1e-9);
        // Above the duct height the duct's all-around reach applies.
        assert!((env.radius_at_z(env.duct_z_mm + 0.01) - env.duct_x_offset_mm[1]).abs() < 1e-9);
        // The profile never shrinks as the height grows.
        let mut previous = 0.0;
        for step in 0..200 {
            let r = env.radius_at_z(step as f64 * 0.1);
            assert!(r >= previous, "profile shrank at {} mm", step as f64 * 0.1);
            previous = r;
        }
        assert!((env.max_reach_radius() - env.duct_x_offset_mm[1]).abs() < 1e-9);
    }

    #[test]
    fn min_height_inverts_the_profile() {
        let env = bambu_a1_envelope();
        for step in 1..200 {
            let h = step as f64 * 0.1;
            let r = env.radius_at_z(h);
            if r <= env.tip_land_radius_mm {
                continue;
            }
            let back = env.min_height_for_radius(r);
            assert!(back <= h + 1e-9, "inverting {h} mm ({r} mm) gave {back} mm");
        }
    }
}