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

/// The sweep may be published only when a person has turned the feature on,
/// confirmed the warning, and the part was accepted. All three are required.
/// Returning true still does not open a socket.
pub fn may_transmit(enabled: bool, user_confirmed: bool, accepted: bool) -> bool {
    enabled && user_confirmed && accepted
}

/// A bed in the coordinate system the sliced G-code uses.
///
/// Y increases toward the back of the machine. On the A1 the bed is what
/// moves in Y, so a sweep toward decreasing Y pushes a part off the front.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MachineBed {
    pub x_min: f64,
    pub y_min: f64,
    pub width: f64,
    pub depth: f64,
    pub z_max: f64,
    pub bed_slinger: bool,
}

impl MachineBed {
    /// The A1's printable area: 256 mm square, origin at the front left.
    pub fn a1() -> Self {
        Self {
            x_min: 0.0,
            y_min: 0.0,
            width: 256.0,
            depth: 256.0,
            z_max: 256.0,
            bed_slinger: true,
        }
    }

    /// The same plate with the origin at the centre, which is what a profile
    /// sets when `machine_center_is_zero` is true.
    pub fn centred(width: f64, depth: f64, z_max: f64) -> Self {
        Self {
            x_min: -width * 0.5,
            y_min: -depth * 0.5,
            width,
            depth,
            z_max,
            bed_slinger: true,
        }
    }

    fn back(self) -> f64 {
        self.y_min + self.depth
    }
}

/// The part, in the same coordinates as [`MachineBed`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartBox {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
    pub min_z: f64,
    pub max_z: f64,
}

impl PartBox {
    pub fn height(self) -> f64 {
        (self.max_z - self.min_z).max(0.0)
    }

    pub fn width(self) -> f64 {
        (self.max_x - self.min_x).max(0.0)
    }

    pub fn depth(self) -> f64 {
        (self.max_y - self.min_y).max(0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SweepRequest {
    pub printer_id: String,
    pub bed: MachineBed,
    pub part: PartBox,
    /// `none`, `skirt`, or `brim`, as the slice resolved it.
    pub adhesion: String,
    /// Degrees. The bed heater is switched off and the routine waits for this.
    pub cool_c: f64,
    /// Skip the "A1 only" gate. The geometry checks still run. Tests of a
    /// later machine use this; the window does not.
    pub allow_other_machines: bool,
}

impl SweepRequest {
    pub fn a1(part: PartBox) -> Self {
        Self {
            printer_id: "bambu_lab_a1".into(),
            bed: MachineBed::a1(),
            part,
            adhesion: "brim".into(),
            cool_c: 28.0,
            allow_other_machines: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SweepDecision {
    Refuse { reasons: Vec<String> },
    Accept { gcode: String, lines: Vec<String> },
}

impl SweepDecision {
    pub fn accepted(&self) -> bool {
        matches!(self, Self::Accept { .. })
    }

    pub fn gcode(&self) -> Option<&str> {
        match self {
            Self::Accept { gcode, .. } => Some(gcode),
            Self::Refuse { .. } => None,
        }
    }
}

/// Automatic removal is offered for the A1 only. The A1 mini, the P2S and
/// everything else are refused here; the generator itself still takes a
/// [`MachineBed`] so a later machine can be tried behind `allow_other_machines`.
pub fn offered_for(printer_id: &str) -> bool {
    let id = printer_id
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_");
    id == "bambu_lab_a1" || id == "a1" || id == "bambulab_a1"
}

const MARGIN: f64 = 8.0;
const MAX_HEIGHT: f64 = 25.0;
const MIN_SPAN: f64 = 15.0;
const MAX_WIDTH: f64 = 180.0;
const SWEEP_Z: f64 = 1.0;

/// Build the removal routine, or a list of reasons it was refused.
///
/// The string is the end G-code. `lines` are the same motion and temperature
/// commands, without comments, for a caller that sends them one at a time
/// after the print. Neither is transmitted by this function.
pub fn sweep(request: &SweepRequest) -> SweepDecision {
    let mut reasons = Vec::new();
    if !request.allow_other_machines && !offered_for(&request.printer_id) {
        reasons.push(format!(
            "Automatic removal is only offered for the A1. This machine is {}.",
            request.printer_id
        ));
    }
    if !request.bed.bed_slinger {
        reasons.push(
            "This sweep pushes with the bed in Y, which is how the A1 moves. It is refused on a \
             machine whose bed does not travel."
                .into(),
        );
    }
    let part = request.part;
    let height = part.height();
    if height > MAX_HEIGHT {
        reasons.push(format!(
            "This part is {height:.0} mm tall. The nozzle only pushes the base, and a part taller \
             than {MAX_HEIGHT:.0} mm can tip into the toolhead."
        ));
    }
    if height < 2.0 {
        reasons.push(format!(
            "This part is {height:.1} mm tall. That is too short for the nozzle to catch it."
        ));
    }
    let span = part.width().min(part.depth());
    if span < MIN_SPAN {
        reasons.push(format!(
            "This part is {span:.0} mm across. The nozzle can miss a part smaller than \
             {MIN_SPAN:.0} mm, or push it sideways instead of off the front."
        ));
    }
    if part.width() > MAX_WIDTH {
        reasons.push(format!(
            "This part is {:.0} mm wide. A nozzle sweep cannot clear a part wider than \
             {MAX_WIDTH:.0} mm on this bed and still stay away from the sides.",
            part.width()
        ));
    }
    if part.min_z > 1.5 {
        reasons.push("The part is not sitting on the plate, so the sweep would miss it.".into());
    }
    let adhesion = request.adhesion.trim().to_ascii_lowercase();
    if adhesion == "none" || adhesion.is_empty() {
        reasons.push(
            "Adhesion is set to none. The part may not be where the sweep expects, so Shale \
             will not push it."
                .into(),
        );
    }
    let bed = request.bed;
    let front = bed.y_min;
    let back = bed.back();
    if part.min_x < bed.x_min
        || part.max_x > bed.x_min + bed.width
        || part.min_y < front
        || part.max_y > back
    {
        reasons.push("The part does not sit inside the printable area.".into());
    }
    if part.max_y + MARGIN > back - MARGIN {
        reasons.push(
            "There is no room to park the nozzle behind this part. Move it forward on the plate."
                .into(),
        );
    }
    if !reasons.is_empty() {
        return SweepDecision::Refuse { reasons };
    }

    let travel_z = (height + 4.0).clamp(10.0, 40.0);
    let behind_y = (part.max_y + MARGIN).min(back - MARGIN);
    let front_y = front + MARGIN;
    let park_y = back - MARGIN;
    let centre_x = (part.min_x + part.max_x) * 0.5;
    let mut xs = Vec::new();
    let start = part.min_x + MARGIN;
    let end = part.max_x - MARGIN;
    if end <= start {
        xs.push(centre_x);
    } else {
        let mut x = start;
        while x < end - 0.5 {
            xs.push(x);
            x += 12.0;
        }
        xs.push(end);
    }
    let cool = request.cool_c.clamp(20.0, 40.0);
    let mut text = String::new();
    text.push_str("; Shale experimental print removal for the A1\n");
    text.push_str(
        "; Untested on real hardware. Stay with the printer the first time and be ready to switch it off.\n",
    );
    text.push_str(
        "; The nozzle parks above the back of the plate, drops just above the surface behind the part, and the bed moves so the part is pushed off the front.\n",
    );
    text.push_str("M140 S0\n");
    text.push_str(&format!("M190 S{}\n", mm_whole(cool)));
    text.push_str("G90\n");
    text.push_str(&format!("G1 Z{} F300\n", mm(travel_z)));
    for (index, x) in xs.iter().enumerate() {
        text.push_str(&format!("; pass {} of {}\n", index + 1, xs.len()));
        text.push_str(&format!(
            "G1 X{} Y{} Z{} F3000\n",
            mm(*x),
            mm(behind_y),
            mm(travel_z)
        ));
        text.push_str(&format!("G1 Z{} F300\n", mm(SWEEP_Z)));
        text.push_str(&format!("G1 Y{} F600\n", mm(front_y)));
        text.push_str(&format!("G1 Z{} F300\n", mm(travel_z)));
    }
    text.push_str("; park above the back of the plate\n");
    text.push_str(&format!(
        "G1 X{} Y{} Z{} F3000\n",
        mm(centre_x),
        mm(park_y),
        mm(travel_z)
    ));
    let lines = text
        .lines()
        .filter(|line| {
            let line = line.trim();
            line.starts_with('G') || line.starts_with('M')
        })
        .map(|line| line.to_string())
        .collect();
    SweepDecision::Accept { gcode: text, lines }
}

fn mm(value: f64) -> String {
    format!("{value:.1}")
}

fn mm_whole(value: f64) -> String {
    format!("{}", value.round() as i64)
}

/// Put the routine at the end of a sliced file. The slice itself is unchanged
/// apart from that suffix.
pub fn append_routine(gcode: &str, routine: &str) -> String {
    let mut out = gcode.trim_end().to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(routine.trim_end());
    out.push('\n');
    out
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

    fn example_part() -> PartBox {
        PartBox {
            min_x: 100.0,
            max_x: 140.0,
            min_y: 80.0,
            max_y: 130.0,
            min_z: 0.0,
            max_z: 12.0,
        }
    }

    #[test]
    fn an_example_a1_part_gets_this_exact_routine() {
        let decision = sweep(&SweepRequest::a1(example_part()));
        let SweepDecision::Accept { gcode, lines } = decision else {
            panic!("the example part should be accepted");
        };
        let expected = "\
; Shale experimental print removal for the A1
; Untested on real hardware. Stay with the printer the first time and be ready to switch it off.
; The nozzle parks above the back of the plate, drops just above the surface behind the part, and the bed moves so the part is pushed off the front.
M140 S0
M190 S28
G90
G1 Z16.0 F300
; pass 1 of 3
G1 X108.0 Y138.0 Z16.0 F3000
G1 Z1.0 F300
G1 Y8.0 F600
G1 Z16.0 F300
; pass 2 of 3
G1 X120.0 Y138.0 Z16.0 F3000
G1 Z1.0 F300
G1 Y8.0 F600
G1 Z16.0 F300
; pass 3 of 3
G1 X132.0 Y138.0 Z16.0 F3000
G1 Z1.0 F300
G1 Y8.0 F600
G1 Z16.0 F300
; park above the back of the plate
G1 X120.0 Y248.0 Z16.0 F3000
";
        assert_eq!(gcode, expected);
        assert!(lines.iter().any(|line| line.starts_with("M190 S28")));
        assert!(lines.iter().any(|line| line == "G1 Y8.0 F600"));
        assert!(!may_transmit(false, true, true));
        assert!(!may_transmit(true, false, true));
        assert!(!may_transmit(true, true, false));
        assert!(may_transmit(true, true, true));
        let appended = append_routine("; slice\nM84\n", &gcode);
        assert!(appended.starts_with("; slice\nM84\n\n; Shale experimental"));
    }

    #[test]
    fn a_part_that_is_too_tall_small_wide_or_loose_is_refused_with_a_reason() {
        let mut tall = SweepRequest::a1(example_part());
        tall.part.max_z = 40.0;
        let SweepDecision::Refuse { reasons } = sweep(&tall) else {
            panic!("tall");
        };
        assert!(reasons[0].contains("40 mm tall"), "{reasons:?}");

        let mut small = SweepRequest::a1(example_part());
        small.part.max_x = 108.0;
        let SweepDecision::Refuse { reasons } = sweep(&small) else {
            panic!("small");
        };
        assert!(
            reasons.iter().any(|reason| reason.contains("across")),
            "{reasons:?}"
        );

        let mut wide = SweepRequest::a1(example_part());
        wide.part.min_x = 10.0;
        wide.part.max_x = 220.0;
        let SweepDecision::Refuse { reasons } = sweep(&wide) else {
            panic!("wide");
        };
        assert!(
            reasons.iter().any(|reason| reason.contains("wide")),
            "{reasons:?}"
        );

        let mut loose = SweepRequest::a1(example_part());
        loose.adhesion = "none".into();
        let SweepDecision::Refuse { reasons } = sweep(&loose) else {
            panic!("loose");
        };
        assert!(
            reasons.iter().any(|reason| reason.contains("Adhesion")),
            "{reasons:?}"
        );
    }

    #[test]
    fn the_p2s_is_refused_and_the_generator_still_runs_for_a_named_test_bed() {
        assert!(!offered_for("bambu_lab_p2s"));
        assert!(!offered_for("bambu_lab_a1_mini"));
        assert!(offered_for("bambu_lab_a1"));
        let mut p2s = SweepRequest::a1(example_part());
        p2s.printer_id = "bambu_lab_p2s".into();
        p2s.bed.bed_slinger = false;
        let SweepDecision::Refuse { reasons } = sweep(&p2s) else {
            panic!("p2s");
        };
        assert!(reasons
            .iter()
            .any(|reason| reason.contains("only offered for the A1")));
        assert!(reasons.iter().any(|reason| reason.contains("bed")));

        let mut later = SweepRequest::a1(example_part());
        later.printer_id = "test_slinger".into();
        later.allow_other_machines = true;
        assert!(sweep(&later).accepted());
    }
}
