//! Shale's core library: printer profiles, the setting resolver, meshes, and
//! the CuraEngine process boundary.
//!
//! The desktop UI and the MCP server both call this crate. Nothing here
//! depends on Tauri or on Cura's Uranium / Qt frontend. CuraEngine itself stays
//! a separate unmodified process behind [`engine::SlicingEngine`].

pub mod bambu;
pub mod cad;
pub mod calibrate;
pub mod cut;
pub mod catalog;
mod definition;
pub mod engine;
pub mod formula;
pub mod gcode;
mod instance;
pub mod instructions;
pub mod job;
mod material;
pub mod exchange;
pub mod layout;
pub mod measure;
pub mod mesh;
pub mod orient;
pub mod project;
pub mod removal;
pub mod resolver;
pub mod session;
pub mod paint;
pub mod supports;
mod value;

pub use cad::{suggest_chord_mm, ImportReport, ImportedMesh, TessellationTolerance};
pub use catalog::{Catalog, MaterialInfo, PrinterInfo, QualityInfo, ResourcePaths, VariantInfo};
pub use engine::{
    ArcusEngine, CliEngine, Engine, EngineBackend, EngineProgress, LiveLayer, SlicingEngine,
};
pub use error::Error;
pub use gcode::Toolpath;
pub use instructions::{CustomInstruction, GcodeFlavour};
pub use job::{compare_support, slice, PlacedModel, SliceOutcome};
pub use mesh::{
    apply_transform, bake, drop_to_bed, load_mesh, load_mesh_reporting, load_mesh_with,
    load_named_bytes, place_on_bed, Transform, TriangleMesh,
};
pub use project::{
    forget_recent, load_recents, now_ms, read_project, remember_recent, LoadedProject, ProjectData,
    ProjectModel, RecentEntry,
};
pub use resolver::{ProfileSummary, ResolveRequest, ResolvedSettings, UserSettings};
pub use session::{JobSliceSummary, Session, SettingsPatch, SettingsView, StoredInstruction};
pub use supports::{
    analyse, overlays, propose, set_enabled, Analysis, FeatureKind, RegionRole, SupportComparison,
    SupportOverlay, SupportParams, SupportPlan, SupportRegion,
};
pub use value::Value;

mod error;
