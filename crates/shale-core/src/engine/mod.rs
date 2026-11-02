//! The slicing engine boundary.
//!
//! `SlicingEngine` is the seam between Shale and CuraEngine. The MVP ships
//! [`cli::CliEngine`]. [`arcus::ArcusEngine`] is the intended replacement: the
//! same request and progress callback, with layer polygons and progress coming
//! from the socket instead of from a G-code file.

mod arcus;
pub mod cli;

pub use arcus::ArcusEngine;
pub use cli::CliEngine;

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::Error;

/// A mesh that is not the part. `settings` are applied to this object only,
/// which is how the 5.13 CLI marks a support enforcer or a blocker.
#[derive(Clone, Debug)]
pub struct EngineMesh {
    pub path: PathBuf,
    pub settings: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct EngineRequest {
    pub global: BTreeMap<String, String>,
    pub extruders: Vec<BTreeMap<String, String>>,
    pub models: Vec<PathBuf>,
    pub modifiers: Vec<EngineMesh>,
    pub output_gcode: PathBuf,
    /// Used by the CLI backend to turn `;LAYER:` lines in the output file
    /// into a progress fraction. Arcus will ignore it and use `Progress` messages.
    pub estimated_layers: u32,
}

#[derive(Clone, Debug)]
pub struct EngineProgress {
    pub fraction: f32,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct EngineOutput {
    pub gcode_path: PathBuf,
    pub log_tail: String,
    pub print_time_seconds: Option<f64>,
}

pub trait SlicingEngine {
    fn slice(
        &self,
        request: &EngineRequest,
        on_progress: &mut dyn FnMut(EngineProgress),
    ) -> Result<EngineOutput, Error>;
}
