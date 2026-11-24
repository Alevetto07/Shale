//! Shared types between `shale-core::supports` and the auto planner.

use serde::{Deserialize, Serialize};

/// Subset of [`shale_core::supports::Span`] passed into the non-planar crate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SupportSpanInput {
    pub kind: String,
    pub length_mm: f64,
    pub width_mm: f64,
    pub area_mm2: f64,
    pub z_mm: f64,
    pub angle_from_vertical_deg: f64,
    pub anchor_ends: u8,
    pub sample_min: [f64; 3],
    pub sample_max: [f64; 3],
}

impl SupportSpanInput {
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        (self.sample_min, self.sample_max)
    }
}