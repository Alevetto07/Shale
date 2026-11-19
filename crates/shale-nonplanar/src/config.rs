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
}

impl Default for NonplanarOptions {
    fn default() -> Self {
        Self {
            mode: NonplanarMode::Off,
            max_angle_deg: 10.0,
            grid_step_mm: 0.35,
            band_thickness_mm: None,
            ramp_thickness_mm: None,
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

    /// Non-planar is only supported on the Bambu A1 family in v0.2.
    pub fn allowed_for_printer(printer_id: &str) -> bool {
        let id = printer_id.to_ascii_lowercase();
        id.contains("bambu_lab_a1") || id == "bambu_lab_a1_mini"
    }

    pub fn active_for_printer(&self, printer_id: &str) -> bool {
        self.mode.active() && Self::allowed_for_printer(printer_id)
    }
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
}