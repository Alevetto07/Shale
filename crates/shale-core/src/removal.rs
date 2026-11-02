//! Camera-based print removal, not wired to a printer.
//!
//! The gate is off unless the caller passes `enabled`. Shale does not open a
//! webcam, does not talk to Moonraker or OctoPrint, and does not send the
//! removal sequence. A later pass can fill in the detect and run steps behind
//! the same types. See `docs/print-removal.md`.

use serde::{Deserialize, Serialize};

/// Where a finished-print camera would come from. Stored, not contacted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraKind {
    /// Moonraker `/webcam/?action=snapshot` or a stream URL from the config.
    Moonraker,
    /// OctoPrint `/webcam/?action=snapshot`.
    OctoPrint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraEndpoint {
    pub kind: CameraKind,
    pub url: String,
}

/// What the operator would run after the bed is seen to be occupied by a
/// finished print, then clear. Lines are data. Nothing here transmits them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalSequence {
    pub name: String,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalGate {
    /// Default. No camera is opened and no sequence can run.
    Disabled,
    /// The flag is on, but this build still will not move a machine.
    Armed { camera: CameraEndpoint },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemovalPlan {
    pub gate: RemovalGate,
    pub sequence: RemovalSequence,
    /// Always false in this stub. A future runner must refuse to act when
    /// this is false.
    pub will_move_printer: bool,
    pub note: String,
}

pub fn gate_from_flag(enabled: bool, camera: Option<CameraEndpoint>) -> RemovalGate {
    if !enabled {
        return RemovalGate::Disabled;
    }
    RemovalGate::Armed {
        camera: camera.unwrap_or(CameraEndpoint {
            kind: CameraKind::Moonraker,
            url: String::new(),
        }),
    }
}

/// `SHALE_PRINT_REMOVAL=1` arms the stub. Any other value, including unset,
/// leaves it disabled. Arming still does not contact a printer.
pub fn enabled_from_env() -> bool {
    matches!(
        std::env::var("SHALE_PRINT_REMOVAL").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

pub fn plan(
    enabled: bool,
    camera: Option<CameraEndpoint>,
    sequence: RemovalSequence,
) -> RemovalPlan {
    let gate = gate_from_flag(enabled, camera);
    let note = match &gate {
        RemovalGate::Disabled => {
            "print removal is off. set SHALE_PRINT_REMOVAL=1 to arm the stub. no printer commands are sent.".to_string()
        }
        RemovalGate::Armed { .. } => {
            "stub armed. the camera is not opened and the sequence is not sent.".to_string()
        }
    };
    RemovalPlan {
        gate,
        sequence,
        will_move_printer: false,
        note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RemovalSequence {
        RemovalSequence {
            name: "sweep".into(),
            lines: vec!["G0 X10 Y200".into(), "G0 X200 Y200".into()],
        }
    }

    #[test]
    fn removal_stays_off_and_never_moves_a_printer() {
        let plan = plan(false, None, sample());
        assert_eq!(plan.gate, RemovalGate::Disabled);
        assert!(!plan.will_move_printer);
        assert!(plan.note.contains("off"));
        assert!(!enabled_from_env());
    }

    #[test]
    fn arming_the_flag_still_does_not_run_the_sequence() {
        let plan = plan(
            true,
            Some(CameraEndpoint {
                kind: CameraKind::OctoPrint,
                url: "http://printer.local/webcam/?action=snapshot".into(),
            }),
            sample(),
        );
        assert!(matches!(plan.gate, RemovalGate::Armed { .. }));
        assert!(!plan.will_move_printer);
        assert!(plan.note.contains("not sent"));
        assert!(!enabled_from_env() || std::env::var("SHALE_PRINT_REMOVAL").is_ok());
    }
}
