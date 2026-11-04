//! The slicing engine boundary.
//!
//! `SlicingEngine` is the seam between Shale and CuraEngine. [`Engine`] picks
//! [`cli::CliEngine`] or [`arcus::ArcusEngine`] at runtime. Arcus is the
//! socket: progress and layer polygons come from protobuf messages. The CLI
//! stays available when `SHALE_ENGINE=cli` or the window's backend control
//! says so.

mod arcus;
pub mod cli;
mod proto;

pub use arcus::ArcusEngine;
pub use cli::CliEngine;

use std::env;

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
    pub models: Vec<EngineMesh>,
    pub modifiers: Vec<EngineMesh>,
    pub output_gcode: PathBuf,
    /// Used by the CLI backend to turn `;LAYER:` lines in the output file
    /// into a progress fraction. Arcus will ignore it and use `Progress` messages.
    pub estimated_layers: u32,
}

/// One layer's toolpaths, pushed through the progress callback while Arcus
/// is still slicing. The CLI backend leaves this empty.
#[derive(Clone, Debug)]
pub struct LivePath {
    pub kind: String,
    pub points: Vec<[f32; 3]>,
}

#[derive(Clone, Debug)]
pub struct LiveLayer {
    pub index: i32,
    pub z: f32,
    pub paths: Vec<LivePath>,
}

#[derive(Clone, Debug)]
pub struct EngineProgress {
    pub fraction: f32,
    pub message: String,
    pub layer: Option<LiveLayer>,
}

impl EngineProgress {
    pub fn new(fraction: f32, message: impl Into<String>) -> Self {
        Self {
            fraction,
            message: message.into(),
            layer: None,
        }
    }
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

/// Which process protocol to use for the next slice. Both talk to the same
/// unmodified CuraEngine binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineBackend {
    Cli,
    Arcus,
}

impl EngineBackend {
    pub fn parse(value: &str) -> Self {
        if value.eq_ignore_ascii_case("arcus") {
            Self::Arcus
        } else {
            Self::Cli
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Arcus => "arcus",
        }
    }
}

/// CLI and Arcus share the binary path. The backend is chosen per slice.
#[derive(Clone, Debug)]
pub struct Engine {
    pub cli: CliEngine,
    pub backend: EngineBackend,
}

impl Engine {
    pub fn from_env() -> Self {
        let backend = env::var("SHALE_ENGINE").unwrap_or_else(|_| "cli".into());
        Self {
            cli: CliEngine::from_env(),
            backend: EngineBackend::parse(&backend),
        }
    }

    pub fn available(&self) -> bool {
        self.cli.available()
    }
}

impl SlicingEngine for Engine {
    fn slice(
        &self,
        request: &EngineRequest,
        on_progress: &mut dyn FnMut(EngineProgress),
    ) -> Result<EngineOutput, Error> {
        match self.backend {
            EngineBackend::Cli => self.cli.slice(request, on_progress),
            EngineBackend::Arcus => ArcusEngine::new(&self.cli).slice(request, on_progress),
        }
    }
}
