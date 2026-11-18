use serde::{Deserialize, Serialize};

/// Experimental non-planar modes. Only [`NonplanarMode::CurvedTops`] is
/// implemented in v0.2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NonplanarMode {
    #[default]
    Off,
    CurvedTops,
}

impl NonplanarMode {
    pub fn from_cli_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "off" | "none" | "" => Some(Self::Off),
            "curved-tops" | "curved_tops" | "curvedtops" => Some(Self::CurvedTops),
            _ => None,
        }
    }

    pub fn active(self) -> bool {
        matches!(self, Self::CurvedTops)
    }
}

/// User-facing options for the non-planar pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NonplanarOptions {
    pub mode: NonplanarMode,
    /// Maximum surface slope above the bed, in degrees (default 10).
    pub max_angle_deg: f64,
    /// Grid step for the height field, in millimetres (default 0.35).
    pub grid_step_mm: f64,
    /// Thickness of the curved top band, in millimetres (default derived from layer height).
    pub band_thickness_mm: Option<f64>,
    /// Ramp thickness at the bottom of the band, in millimetres.
    pub ramp_thickness_mm: Option<f64>,
    /// Integration tests only: leave CuraEngine output for an outer post-pass.
    #[serde(default)]
    pub defer_post_pass: bool,
}

impl Default for NonplanarOptions {
    fn default() -> Self {
        Self {
            mode: NonplanarMode::Off,
            max_angle_deg: 10.0,
            grid_step_mm: 0.35,
            band_thickness_mm: None,
            ramp_thickness_mm: None,
            defer_post_pass: false,
        }
    }
}

impl NonplanarOptions {
    pub fn effective_band_thickness(&self, layer_height: f64) -> f64 {
        self.band_thickness_mm
            .unwrap_or((layer_height * 8.0).clamp(0.8, 4.0))
    }

    pub fn effective_ramp_thickness(&self, layer_height: f64) -> f64 {
        self.ramp_thickness_mm
            .unwrap_or((layer_height * 2.0).clamp(0.16, 0.8))
    }

    /// Non-planar curved tops are enabled for Bambu printers that ship a head envelope.
    pub fn allowed_for_printer(printer_id: &str) -> bool {
        let id = printer_id.to_ascii_lowercase();
        id.starts_with("bambu_lab_")
            && crate::envelope::head_envelope_for_printer(&id).is_some()
    }

    pub fn active_for_printer(&self, printer_id: &str) -> bool {
        self.mode.active() && Self::allowed_for_printer(printer_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bambu_catalogue_printers_allowed() {
        for id in crate::envelope::envelope_catalogue() {
            assert!(NonplanarOptions::allowed_for_printer(id), "{id}");
        }
        assert!(!NonplanarOptions::allowed_for_printer("demo_ender"));
    }
}

/// Moves in the grafted **curved suffix** whose live Z is still below `global_band_floor_z()`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CurvedSuffixBelowFloorMoves {
    pub band_floor_z: f64,
    /// Z slack vs per-column `z_band_start` when classifying extrusion as curved band edge vs planar shell.
    pub shell_extrusion_tolerance_mm: f64,
    pub travel: usize,
    pub z_hop: usize,
    pub wipe_retract: usize,
    /// G1+E with Z below global floor but still above `z_band_start(x,y)` (inverse-mapped curved wall).
    pub band_edge_extrusion: usize,
    /// G1+E at or below `z_band_start(x,y) + tolerance` after curved shell has begun — must be 0.
    pub shell_extrusion_violations: usize,
    pub mcode_or_other: usize,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_shell_violation: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PlanarEquivalenceResult {
    pub ok: bool,
    pub band_floor_z: f64,
    pub planar_moves_below_band: usize,
    pub curved_moves_below_band: usize,
    pub mismatched_moves: usize,
    pub first_mismatch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curved_suffix_below_floor: Option<CurvedSuffixBelowFloorMoves>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SafetyViolation {
    pub kind: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SafetyReport {
    pub ok: bool,
    pub printer_id: String,
    pub max_angle_deg: f64,
    pub band_floor_z: f64,
    pub z_limit_mm: f64,
    pub max_z_seen: f64,
    pub z_within_limits: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_extrusion_slope_deg: Option<f64>,
    /// Effective cap: `max_angle_deg` + [`crate::safety::SLOPE_CAP_TOLERANCE_DEG`].
    pub slope_cap_deg: f64,
    pub slope_within_cap: bool,
    pub extrusion_jump_count: usize,
    pub feed_cap_violations: usize,
    pub travel_collisions_remaining: usize,
    pub envelope_checks_enabled: bool,
    pub planar_equivalence: PlanarEquivalenceResult,
    pub collision_adjustments: Vec<CollisionAdjustment>,
    pub violations: Vec<SafetyViolation>,
}

/// One collision-driven change recorded during flattening.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CollisionAdjustment {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub action: String,
}

/// Summary written into G-code comments and returned to callers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NonplanarReport {
    pub mode: String,
    pub max_angle_deg: f64,
    pub top_islands: usize,
    pub islands_flattened: usize,
    pub fallback_to_planar: bool,
    pub notes: Vec<String>,
    #[serde(default)]
    pub envelope_id: String,
    #[serde(default)]
    pub collision_adjustments: Vec<CollisionAdjustment>,
    #[serde(default)]
    pub planar_equivalence_ok: bool,
    #[serde(default)]
    pub safety: Option<SafetyReport>,
}