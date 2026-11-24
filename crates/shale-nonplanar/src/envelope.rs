//! Toolhead envelopes: the swept volume a printer's head occupies above the nozzle tip.
//!
//! The profile is a **silhouette**: at height `h` above the nozzle tip the head
//! occupies every point within `radius_at_z(h)` of the nozzle axis. Material
//! printed earlier strikes the head when it lies inside that silhouette, which
//! is the same test as "its height above the tip exceeds
//! [`HeadEnvelope::max_material_height_at`] for its horizontal distance".

use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

/// Material closer than this to the tip is the bead being laid, not an obstacle.
///
/// The nozzle rides on top of its own bead, so the freshly deposited layer is
/// always within a layer height of the tip; only material *above* the tip by
/// more than this tolerance can be a toolhead strike.
///
/// Margin note (measured, 0.3 mm land, 0.35 mm grid): a surface climbing at
/// angle `a` leaves material `(0.3 + 0.35) * tan(a)` above the tip in the
/// nearest grid cell, so a band steeper than roughly 6 deg begins to flag its
/// own beads — 0 hits at 5 deg, 307 at 8 deg and 2244 of 4600 extruding moves
/// at 10 deg; with 0.1 mm the same bands are clean up to 12 deg. Raise this
/// constant if curved bands must stay out of the planar fallback at the full
/// slope cap.
///
/// Decision (Shale Builder, 24 Nov 2026): 0.1 mm, i.e. half of a 0.2 mm layer.
/// Contact closer than that is the nozzle land riding over the neighbouring
/// bead of the same curved band (an ironing-like squish), not a head strike;
/// it keeps 10 deg bands, the default cap, clear of false collisions.
pub const COLLISION_TOLERANCE_MM: f64 = 0.1;

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
    /// Height of the nozzle taper: above this the heater block / sock / duct rule.
    pub nozzle_height_mm: f64,
    /// Flat land around the tip; the first permitted contact.
    pub tip_land_radius_mm: f64,
    pub nozzle_cone_half_angle_deg: f64,
    pub body_radius_mm: f64,
    pub body_top_z_mm: f64,
    pub sock_outer_radius_mm: f64,
    pub sock_top_z_mm: f64,
    /// Fan-duct X offsets `[front, back]`. The duct is checked all around
    /// (conservative) using the larger offset.
    pub duct_x_offset_mm: [f64; 2],
    pub duct_z_mm: f64,
    pub default_max_clearance_angle_deg: f64,
}

impl HeadEnvelope {
    /// Radius of the heater block / silicone sock column.
    pub fn body_sock_radius_mm(&self) -> f64 {
        self.body_radius_mm.max(self.sock_outer_radius_mm)
    }

    /// Widest horizontal reach of the head: nothing farther than this can be hit.
    pub fn horizontal_reach_mm(&self) -> f64 {
        self.body_sock_radius_mm()
            .max(self.duct_x_offset_mm[1].abs())
    }

    /// Highest material (mm above the nozzle tip) allowed at horizontal
    /// distance `d` without touching the toolhead.
    ///
    /// Three rules, each a column that starts at its own height:
    ///
    /// * nozzle cone — collides at `h > tol` when `d <= tip_land + h*tan(half_angle)`;
    /// * heater block / sock — collides at `h > nozzle_height` inside `max(body, sock)`;
    /// * fan duct — collides at `h > duct_z` inside the larger duct offset.
    ///
    /// The value returned is `f64::INFINITY` where no rule applies, so a caller
    /// can compare `material_height > max_material_height_at(d)` directly.
    pub fn max_material_height_at(&self, d: f64) -> f64 {
        let d = d.abs();
        let tan = self.nozzle_cone_half_angle_deg.to_radians().tan();
        // The cone starts half a step above the tip: the bead being laid sits
        // exactly at tip height and must never count as an obstacle.
        let mut limit = if d <= self.tip_land_radius_mm {
            COLLISION_TOLERANCE_MM
        } else if tan > 0.0 {
            ((d - self.tip_land_radius_mm) / tan).max(COLLISION_TOLERANCE_MM)
        } else {
            f64::INFINITY
        };
        if d <= self.body_sock_radius_mm() {
            limit = limit.min(self.nozzle_height_mm);
        }
        if d <= self.duct_x_offset_mm[1].abs() {
            limit = limit.min(self.duct_z_mm);
        }
        limit
    }

    /// Would material `height_above_tip_mm` tall at `distance_mm` from the
    /// nozzle axis strike this head?
    pub fn material_collides(&self, height_above_tip_mm: f64, distance_mm: f64) -> bool {
        height_above_tip_mm > self.max_material_height_at(distance_mm)
    }

    /// Horizontal reach of the envelope at a given height above the nozzle tip.
    ///
    /// This is the inverse of [`Self::max_material_height_at`]: the head only
    /// occupies `tip_land + h*tan(half_angle)` around the nozzle until the
    /// heater block or the duct starts, so the silhouette steps out at
    /// `nozzle_height_mm` and again at `duct_z_mm`.
    pub fn radius_at_z(&self, z_above_tip: f64) -> f64 {
        if z_above_tip <= 0.0 {
            return self.tip_land_radius_mm;
        }
        let tan = self.nozzle_cone_half_angle_deg.to_radians().tan().max(0.0);
        let mut r = self.tip_land_radius_mm + z_above_tip * tan;
        if z_above_tip > self.nozzle_height_mm {
            r = r.max(self.body_sock_radius_mm());
        }
        if z_above_tip > self.duct_z_mm {
            r = r.max(self.duct_x_offset_mm[1].abs());
        }
        r
    }
}

/// A printer with no embedded toolhead envelope cannot be sliced non-planar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedPrinter {
    pub printer_id: String,
}

impl fmt::Display for UnsupportedPrinter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "non-planar needs a toolhead envelope; printer '{}' has none (supported: {})",
            self.printer_id,
            envelope_catalogue().join(", ")
        )
    }
}

impl std::error::Error for UnsupportedPrinter {}

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
            let env: HeadEnvelope = serde_json::from_str(text)
                .unwrap_or_else(|err| panic!("{id} envelope JSON: {err}"));
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

/// The envelope for `printer_id`, or the error the CLI and the job pipeline show.
pub fn require_head_envelope(
    printer_id: &str,
) -> Result<&'static HeadEnvelope, UnsupportedPrinter> {
    head_envelope_for_printer(printer_id).ok_or_else(|| UnsupportedPrinter {
        printer_id: printer_id.to_string(),
    })
}

pub fn bambu_a1_envelope() -> &'static HeadEnvelope {
    head_envelope_for_printer("bambu_lab_a1").expect("embedded A1 envelope")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a1() -> &'static HeadEnvelope {
        head_envelope_for_printer("bambu_lab_a1").expect("a1")
    }

    fn x1c() -> &'static HeadEnvelope {
        head_envelope_for_printer("bambu_lab_x1_carbon").expect("x1c")
    }

    fn cone_height_at(env: &HeadEnvelope, d: f64) -> f64 {
        (d - env.tip_land_radius_mm) / env.nozzle_cone_half_angle_deg.to_radians().tan()
    }

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
    fn a1_tip_land_threshold_is_the_bead_tolerance() {
        let env = a1();
        assert_eq!(env.max_material_height_at(0.0), COLLISION_TOLERANCE_MM);
        assert_eq!(
            env.max_material_height_at(env.tip_land_radius_mm),
            COLLISION_TOLERANCE_MM
        );
        assert!(!env.material_collides(COLLISION_TOLERANCE_MM - 0.01, 0.0));
        assert_eq!(env.max_material_height_at(0.04), COLLISION_TOLERANCE_MM);
        assert!(env.material_collides(COLLISION_TOLERANCE_MM + 0.01, 0.0));
        assert!(env.material_collides(COLLISION_TOLERANCE_MM + 0.01, env.tip_land_radius_mm * 0.5));
    }

    #[test]
    fn a1_nozzle_cone_threshold() {
        let env = a1();
        let d = 1.0;
        let expected = cone_height_at(env, d);
        assert!(
            (env.max_material_height_at(d) - expected).abs() < 1e-9,
            "{expected} vs {}",
            env.max_material_height_at(d)
        );
        assert!(!env.material_collides(expected - 0.01, d));
        assert!(env.material_collides(expected + 0.01, d));
        // The cone keeps growing with height but never below the bead tolerance.
        assert!(env.max_material_height_at(0.5) > env.max_material_height_at(0.35) - 1e-9);
    }

    #[test]
    fn a1_body_threshold_starts_at_nozzle_height() {
        let env = a1();
        let d = 6.0; // inside max(body, sock) = 12, outside the cone at 4.76 mm
        assert!(cone_height_at(env, d) > env.nozzle_height_mm);
        assert_eq!(env.max_material_height_at(d), env.nozzle_height_mm);
        assert!(!env.material_collides(env.nozzle_height_mm - 0.01, d));
        assert!(env.material_collides(env.nozzle_height_mm + 0.01, d));
        // Nothing above the nozzle height touches the head until the body radius.
        assert_eq!(
            env.max_material_height_at(env.body_sock_radius_mm() - 0.01),
            env.nozzle_height_mm
        );
    }

    #[test]
    fn a1_duct_threshold_starts_at_duct_z() {
        let env = a1();
        let d = 15.0; // between body radius 12 and duct reach 26
        assert!(d > env.body_sock_radius_mm() && d <= env.duct_x_offset_mm[1]);
        assert_eq!(env.max_material_height_at(d), env.duct_z_mm);
        assert!(!env.material_collides(env.duct_z_mm - 0.01, d));
        assert!(env.material_collides(env.duct_z_mm + 0.01, d));
        assert_eq!(
            env.max_material_height_at(env.duct_x_offset_mm[1]),
            env.duct_z_mm
        );
    }

    #[test]
    fn a1_beyond_the_duct_only_the_cone_applies() {
        let env = a1();
        let d = 30.0;
        assert!(d > env.duct_x_offset_mm[1]);
        assert!(env.max_material_height_at(d) > 50.0);
        assert!(!env.material_collides(env.duct_z_mm + 1.0, d));
    }

    #[test]
    fn x1c_cone_body_and_duct_thresholds() {
        let env = x1c();
        let cone_d = 1.0;
        let expected = cone_height_at(env, cone_d);
        assert!((env.max_material_height_at(cone_d) - expected).abs() < 1e-9);
        assert!(!env.material_collides(expected - 0.01, cone_d));
        assert!(env.material_collides(expected + 0.01, cone_d));

        let body_d = 8.0;
        assert_eq!(env.max_material_height_at(body_d), env.nozzle_height_mm);
        assert!(!env.material_collides(env.nozzle_height_mm - 0.01, body_d));
        assert!(env.material_collides(env.nozzle_height_mm + 0.01, body_d));

        let duct_d = 20.0;
        assert!(duct_d > env.body_sock_radius_mm());
        assert_eq!(env.max_material_height_at(duct_d), env.duct_z_mm);
        assert!(!env.material_collides(env.duct_z_mm - 0.01, duct_d));
        assert!(env.material_collides(env.duct_z_mm + 0.01, duct_d));
        assert!(env.max_material_height_at(40.0) > 50.0);
    }

    #[test]
    fn radius_at_z_steps_out_at_the_body_and_the_duct() {
        let env = a1();
        assert_eq!(env.radius_at_z(0.0), env.tip_land_radius_mm);
        assert!(env.radius_at_z(1.0) < env.body_sock_radius_mm());
        assert_eq!(
            env.radius_at_z(env.nozzle_height_mm + 0.01),
            env.body_sock_radius_mm()
        );
        assert_eq!(
            env.radius_at_z(env.duct_z_mm + 0.01),
            env.duct_x_offset_mm[1]
        );
        // Below the nozzle height the silhouette is only the cone, not the body.
        assert!(env.radius_at_z(1.0) < env.radius_at_z(env.nozzle_height_mm + 0.01));
    }

    #[test]
    fn require_head_envelope_accepts_catalogue_printers() {
        for id in envelope_catalogue() {
            let env = require_head_envelope(id).expect(id);
            assert_eq!(env.printer_id, id);
        }
        assert!(require_head_envelope("BAMBU_LAB_A1").is_ok());
        assert!(require_head_envelope("bambu_lab_x1c").is_ok());
    }

    #[test]
    fn require_head_envelope_reports_unsupported_printers() {
        let err = require_head_envelope("creality_k1").expect_err("unsupported printer");
        let text = err.to_string();
        assert_eq!(
            text,
            "non-planar needs a toolhead envelope; printer 'creality_k1' has none \
             (supported: bambu_lab_a1, bambu_lab_a1_mini, bambu_lab_p1s, bambu_lab_p1p, \
             bambu_lab_x1_carbon, bambu_lab_h2d)"
        );
        assert_eq!(err.printer_id, "creality_k1");
    }
}
