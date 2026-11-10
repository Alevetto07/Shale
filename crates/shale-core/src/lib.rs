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
pub mod cad;
pub mod calibrate;
pub mod csg;
pub mod cut;
mod manifold_csg;
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
pub mod primitives;
pub mod project;
pub mod queue;
pub mod removal;
pub mod resolver;
pub mod session;
pub mod paint;
pub mod supports;
pub mod template;
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
