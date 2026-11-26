//! Shale's core library: printer profiles, the setting resolver, meshes, and
//! the CuraEngine process boundary.
//!
//! The desktop UI and the MCP server both call this crate. Nothing here
//! depends on Tauri or on Cura's Uranium / Qt frontend. CuraEngine itself stays
//! a separate unmodified process behind [`engine::SlicingEngine`].

// The kernel's signatures carry the geometry they need. A slice is honestly
// described by thirteen parameters (catalog, engine, printer, variant, quality,
// materials, settings, models, instructions, output …); folding them into a
// builder struct for one internal call would hide more than it explains, and the
// alternative — a `Slice` object with setters — is the same information with
// more places to be wrong.
#![allow(clippy::too_many_arguments)]
// Tuples are the currency of the mesh code: `(vertex, normal, patch)` and
// `(index, face, patch)`. Naming every shape would add types that say nothing a
// reader does not already see at the call site.
#![allow(clippy::type_complexity)]

pub mod bambu;
pub mod brim_ears;
pub mod cad;
pub mod calibrate;
pub mod catalog;
pub(crate) mod catalog_cache;
#[cfg(test)]
mod catalog_cache_bench;
pub mod cost;
pub mod csg;
pub mod cut;
mod definition;
pub mod engine;
pub mod exchange;
pub mod formula;
pub mod gcode;
mod instance;
pub mod instructions;
pub mod job;
pub mod layout;
mod manifold_csg;
mod material;
pub mod measure;
pub mod mesh;
pub mod orient;
pub mod paint;
pub mod preset;
pub mod primitives;
pub mod project;
pub mod project_settings;
pub mod queue;
pub mod removal;
pub mod stats;
pub mod resolver;
pub mod session;
pub mod slicer_import;
pub mod suggest;
pub mod supports;
pub mod template;
mod value;

pub use cad::{suggest_chord_mm, ImportReport, ImportedMesh, TessellationTolerance};
pub use catalog::{Catalog, MaterialInfo, PrinterInfo, QualityInfo, ResourcePaths, VariantInfo};
pub use engine::{
    cancel_running_engines,
    ArcusEngine, CliEngine, Engine, EngineBackend, EngineProgress, LiveLayer, SlicingEngine,
};
pub use error::Error;
pub use gcode::Toolpath;
pub use instructions::{CustomInstruction, GcodeFlavour};
pub use job::{compare_support, slice, slice_with_boxes, PlacedModel, SliceOutcome};
pub use mesh::{
    apply_transform, assign_plates, bake, drop_to_bed, load_build_objects, load_mesh,
    load_mesh_reporting, load_mesh_with, load_named_bytes, place_assembly, place_on_bed,
    read_plate_groups, BuildObject, PlateGroup, Transform, TriangleMesh,
};
pub use project_settings::{
    apply_project_settings, read_project_settings, ImportedSetting, ProjectSettings,
    ProjectSettingsImport,
};
pub use project::{
    config_dir, forget_recent, load_recents, now_ms, read_project, remember_recent, write_project,
    LoadedProject, ProjectData, ProjectModel, ProjectSupportBox, RecentEntry,
};
pub use stats::{
    features_from_gcode, from_gcode as slice_stats_from_gcode, ExtruderUse, FeatureUse, SliceStats,
};
pub use resolver::{ProfileSummary, ResolveRequest, ResolvedSettings, UserSettings};
pub use session::{
    AppliedSuggestion, DismissedSuggestion, JobSliceSummary, PreparedSlice, ProjectOutline,
    Session, SettingsPatch, SettingsView, StoredInstruction, VariantComparison,
};
pub use suggest::{
    evaluate as evaluate_suggestions, filament_kind, gcode_facts, present as present_suggestions,
    printer_is_enclosed, FilamentSlot, GcodeFacts, PrinterFacts, Scene, SceneObject, Severity,
    Suggestion, SuggestionFix, SlotQuery, SuggestQuery,
};
pub use supports::{
    analyse, overlays, propose, set_enabled, Analysis, FeatureKind, RegionRole, SupportComparison,
    SupportOverlay, SupportParams, SupportPlan, SupportRegion,
};
pub use value::Value;
pub use shale_nonplanar::{
    check_user_max_angle, count_curved_extrusions, plan_auto_placement, safety_report_from_gcode,
    segment_slope_deg, worst_extrusion_slope_deg, AutoPlacementReport, DetectionReport,
    NonplanarMode, NonplanarOptions, NonplanarReport, PlannedSurface, PlannedSurfaceKind,
    SafetyReport, SurfacePreview, CURVED_EXTRUSION_MIN_SLOPE_DEG, USER_MAX_ANGLE_DEG,
};

/// Removes the pre-v0.1.2 per-install cache under `%LOCALAPPDATA%\\Shale\\cache`.
pub fn remove_legacy_catalog_cache() {
    catalog_cache::remove_legacy_cache_dir();
}

/// Redirects on-disk catalogue cache writes for this process (tests and tooling).
pub fn set_catalog_cache_dir(dir: std::path::PathBuf) {
    catalog_cache::set_cache_dir_override(dir);
}

/// Stable `%TEMP%/shale-test-runs/<component>/catalog-cache` reused by integration tests
/// instead of `tempfile::tempdir()` (which leaves `.tmp*` folders behind when kept in statics).
pub fn integration_test_cache_dir(component: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("shale-test-runs")
        .join(component)
        .join("catalog-cache");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

mod error;
pub mod auto_spans;
pub mod nonplanar_bridge;
