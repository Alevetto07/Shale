//! Automatic non-planar surface detection and placement (v0.2).

mod detect;
mod plan;

pub use detect::{DetectedFeature, DetectedFeatureKind, DetectionReport};
pub use plan::{
    apply_plan_to_field, plan_auto_placement, AutoPlacementReport, PlannedSurface,
    PlannedSurfaceKind, SurfacePreview,
};

#[cfg(test)]
mod tests;