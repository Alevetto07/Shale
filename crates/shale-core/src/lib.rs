//! Shale's core library: printer profiles, the setting resolver, meshes, and
//! the CuraEngine process boundary.
//!
//! The desktop UI and the MCP server both call this crate. Nothing here
//! depends on Tauri or on Cura's Uranium / Qt frontend. CuraEngine itself stays
//! a separate unmodified process behind [`engine::SlicingEngine`].

pub mod catalog;
mod definition;
pub mod engine;
pub mod formula;
pub mod gcode;
mod instance;
pub mod instructions;
pub mod job;
mod material;
pub mod mesh;
pub mod resolver;
pub mod session;
mod value;

pub use catalog::{Catalog, MaterialInfo, PrinterInfo, QualityInfo, ResourcePaths, VariantInfo};
pub use engine::{CliEngine, EngineProgress, SlicingEngine};
pub use error::Error;
pub use gcode::Toolpath;
pub use instructions::{CustomInstruction, GcodeFlavour};
pub use job::{slice, PlacedModel, SliceOutcome};
pub use mesh::{drop_to_bed, load_mesh, place_on_bed, Transform, TriangleMesh};
pub use resolver::{ProfileSummary, ResolveRequest, ResolvedSettings, UserSettings};
pub use session::{JobSliceSummary, Session, SettingsPatch, SettingsView, StoredInstruction};
pub use value::Value;

mod error;
