//! Arcus backend, not implemented in this MVP.
//!
//! Cura's frontend does not pass settings on the command line. It listens on
//! a TCP port and starts `CuraEngine connect <host>:<port>`. libArcus then
//! frames protobuf messages from `Cura.proto`:
//!
//! ```text
//! uint32 header | uint32 size | uint32 type_id | protobuf bytes
//! ```
//!
//! `type_id` is libArcus's private hash of the protobuf `GetTypeName()`
//! (`cura.proto.Slice`, `cura.proto.Progress`, …). Both sides must register
//! the same messages or the hashes will not match. The session itself is a
//! small state machine: Shale would accept the connection, send one `Slice`
//! (global settings, per-extruder settings, and each mesh as raw float
//! buffers), then read `Progress`, `Layer` / `LayerOptimized`, `GCodeLayer`,
//! and `PrintTimeMaterialEstimates` until `SlicingFinished`.
//!
//! That is the right long-term integration. It is a poor MVP default:
//!
//! * Official CuraEngine builds link libArcus and protobuf. A CLI-only binary
//!   is the one that is practical to build and to ship beside Shale, and the
//!   AGPL boundary stays obvious when we do not link the engine.
//! * The type id is not the protobuf tag number; it has to be reproduced
//!   exactly, including the hash, before a single message will decode.
//! * CuraEngine 5.13's command-line `sendProgress` is empty and `-p` is a
//!   leftover no-op, so the CLI cannot stream engine percentages. The CLI
//!   backend therefore estimates progress from `;LAYER:` lines as they are
//!   flushed. Arcus `Progress` messages are the real stream.
//!
//! `CliEngine` implements the same [`super::SlicingEngine`] trait. Replacing
//! it does not change the UI or the resolver.

use super::{EngineOutput, EngineProgress, EngineRequest, SlicingEngine};
use crate::error::Error;

#[derive(Clone, Debug, Default)]
pub struct ArcusEngine;

impl SlicingEngine for ArcusEngine {
    fn slice(
        &self,
        _request: &EngineRequest,
        _on_progress: &mut dyn FnMut(EngineProgress),
    ) -> Result<EngineOutput, Error> {
        Err(Error::Engine(
            "The Arcus backend is not implemented yet. Shale is using the CuraEngine CLI behind the same SlicingEngine trait; see engine/arcus.rs for the protocol and why it was deferred.".into(),
        ))
    }
}
