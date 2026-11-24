use serde::{Deserialize, Serialize};

/// Experimental non-planar modes. Only [`NonplanarMode::CurvedTops`] is
/// implemented in v0.2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NonplanarMode {
    #[default]
    Off,
    /// Apply curved-top deformation to the whole printable top shell.
    CurvedTops,
    /// Detect sloped/curved tops and enable bands only where they help.
    Auto,
}

impl NonplanarMode {
    pub fn from_cli_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "off" | "none" | "" => Some(Self::Off),
            "curved-tops" | "curved_tops" | "curvedtops" => Some(Self::CurvedTops),
            "auto" | "automatic" => Some(Self::Auto),
            _ => None,
        }
    }

    pub fn active(self) -> bool {
        matches!(self, Self::CurvedTops | Self::Auto)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::CurvedTops => "curved-tops",
            Self::Auto => "auto",
        }
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
    /// When the post-pass cannot verify the curved output, fail the slice with an
    /// error instead of emitting the planar baseline. Off by default: a print that
    /// cannot be verified still produces the flat G-code the job already sliced.
    #[serde(default)]
    pub strict: bool,
    /// Let auto plan the underside bands (overhang / cantilever / bridge).
    ///
    /// Off by default. A band under the part has the part itself above it at the
    /// same XY — a deck over a mast, the underside of a T — so neither the head
    /// sweep nor the planar-equivalence check can clear it, and the tilt it needs is
    /// the angle *down* from horizontal, not the slope of a top surface. Auto used
    /// to enable them anyway and then abandon the whole plate when they collided
    /// (M1-B3: the 24 mm dome fell back at the first underside surface). They stay
    /// reachable for experiments, but nothing claims they are verified.
    #[serde(default)]
    pub underside_bands: bool,
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
            strict: false,
            underside_bands: false,
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

    /// Non-planar curved tops are enabled for printers that ship a head envelope.
    pub fn allowed_for_printer(printer_id: &str) -> bool {
        crate::envelope::head_envelope_for_printer(printer_id).is_some()
    }

    pub fn active_for_printer(&self, printer_id: &str) -> bool {
        self.mode.active() && Self::allowed_for_printer(printer_id)
    }

    /// Refuse a non-planar request on a printer without a toolhead envelope.
    ///
    /// Slicing planar is always fine, so `Off` never fails. Anything else needs
    /// the envelope: without it the collision pass cannot tell whether the head
    /// clears the part, and silently slicing planar would hide the request.
    pub fn check_printer(&self, printer_id: &str) -> Result<(), String> {
        if !self.mode.active() {
            return Ok(());
        }
        crate::envelope::require_head_envelope(printer_id)
            .map(|_| ())
            .map_err(|err| err.to_string())
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

    #[test]
    fn check_printer_refuses_only_active_modes_on_unsupported_printers() {
        let active = NonplanarOptions {
            mode: NonplanarMode::Auto,
            ..NonplanarOptions::default()
        };
        for id in crate::envelope::envelope_catalogue() {
            assert!(active.check_printer(id).is_ok(), "{id}");
        }
        let err = active
            .check_printer("creality_k1")
            .expect_err("unsupported");
        assert_eq!(
            err,
            "non-planar needs a toolhead envelope; printer 'creality_k1' has none \
             (supported: bambu_lab_a1, bambu_lab_a1_mini, bambu_lab_p1s, bambu_lab_p1p, \
             bambu_lab_x1_carbon, bambu_lab_h2d)"
        );
        // Planar slicing on the same printer stays allowed.
        assert!(NonplanarOptions::default()
            .check_printer("creality_k1")
            .is_ok());
        assert!(!active.active_for_printer("creality_k1"));
    }
}

/// What the pre-slice slope limiter did to the deformation field.
///
/// The limiter raises the strength schedule monotonically towards planar, so
/// `worst_after_deg` is what the post-pass verification is judged against; the
/// report carries the number of damping passes and the field slope before and
/// after so a reviewer can see the margin instead of trusting a boolean.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SlopeLimitingReport {
    /// Limit applied to level-surface gradients, in degrees.
    pub cap_deg: f64,
    /// Damping passes actually run; 0 means the field was already inside the cap.
    pub passes: usize,
    /// False when the pass budget ran out before the worst slope fit the cap.
    pub converged: bool,
    pub worst_before_deg: f64,
    pub worst_after_deg: f64,
    /// Grid cells whose flattening strength was reduced.
    pub cells_reduced: usize,
    /// Smallest strength left anywhere in the field (1 = untouched, 0 = planar).
    ///
    /// Zero for every auto field by construction — the footprint is a subset of the
    /// top — so this is a diagnostic, not a verdict.
    pub min_strength: f64,
    /// Largest strength left on a column with a printable top.
    ///
    /// This is the collapse verdict: a job is only "flattened away" when nothing
    /// lifts any more.
    #[serde(default)]
    pub max_strength: f64,
    /// Columns still in the deforming footprint after the cap cut.
    #[serde(default)]
    pub footprint_columns: usize,
    /// Columns whose strength had to drop so their transition fit above the bed.
    ///
    /// The ramp is sized from the column's own lift (see
    /// [`crate::deform::DeformField::transition_thickness_for_lift`]); where the
    /// part below the band is too short for it, the lift itself is attenuated
    /// instead of the layer thickness being crushed.
    #[serde(default)]
    pub thickness_attenuated_columns: usize,
}

/// Corrections the inverse map had to make to keep a move inside the slope cap.
///
/// The map used to clamp Z by any amount; M1 restricts it to
/// [`crate::inverse::MAX_INVERSE_CLAMP_MM`], counts every one of those, and
/// leaves larger excess in the G-code for the post-pass to verify (and to reject
/// with a planar fallback).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct InverseMapStats {
    pub clamps: usize,
    pub largest_clamp_mm: f64,
    /// Moves whose slope excess exceeded the clamp budget and were left alone.
    pub unclamped_excess: usize,
}

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

fn default_true() -> bool {
    true
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
    /// Collision hits left after the flattening pass, split by move kind.
    ///
    /// Swept moves that would strike printed material while extruding...
    #[serde(default)]
    pub collisions_extruding: usize,
    /// ...and travel moves that would strike printed material.
    #[serde(default)]
    pub collisions_travel: usize,
    pub travel_collisions_remaining: usize,
    /// Collision hits below the band that the sweep ignored because the file is the
    /// planar baseline there (see [`BELOW_BAND_COLLISION_SLACK_MM`]).
    #[serde(default)]
    pub below_band_collisions_ignored: usize,
    pub envelope_checks_enabled: bool,
    /// Thinnest physical layer the map deposits anywhere (millimetres).
    ///
    /// A deformed layer is `nominal_layer` tall; the inverse map can send it to
    /// less than that, and below `min_layer` the bead has nowhere to go. `None`
    /// for planar jobs, where the slicer's own layer height stands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_physical_layer_mm: Option<f64>,
    /// Thickest physical layer the map deposits anywhere (millimetres).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_physical_layer_mm: Option<f64>,
    /// Every band sample lands inside `[min_layer, max_layer]` (+ tolerance).
    #[serde(default = "default_true")]
    pub layer_thickness_within_bounds: bool,
    /// Band samples the thickness scan took.
    #[serde(default)]
    pub layer_thickness_samples: usize,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_placement: Option<crate::auto::AutoPlacementReport>,
    #[serde(default)]
    pub envelope_id: String,
    #[serde(default)]
    pub collision_adjustments: Vec<CollisionAdjustment>,
    #[serde(default)]
    pub planar_equivalence_ok: bool,
    /// Pre-slice slope limiting result; `None` for jobs that never built a field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope_limiting: Option<SlopeLimitingReport>,
    /// Inverse-map slope clamps that were small enough to apply (<= 0.02 mm each).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inverse_corrections: Option<InverseMapStats>,
    /// Why the curved pass was abandoned for the planar baseline, verbatim.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_reasons: Vec<String>,
    #[serde(default)]
    pub safety: Option<SafetyReport>,
}
