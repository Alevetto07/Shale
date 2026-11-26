//! What an overnight queue is allowed to do on its own — which is nothing,
//! unless a person has turned on the experimental A1 removal path.
//!
//! A queue is a list of jobs and one press that authorises the list. The press
//! covers the first job; every job after it needs a second, explicit press,
//! because when a print finishes the bed still holds the part that was just
//! printed. A queue that advances itself prints into that part, then into the
//! next one, and the operator finds three failed prints instead of one.
//!
//! The state machine lives here, away from the window, so the rule can be
//! tested without a printer. With removal left off, [`QueueRun::observe`] never
//! returns a state that starts a job. The experimental path is a separate
//! setting, default off, and even then a following job is allowed only after
//! the plate check says the bed is empty.

/// What the machine is doing, as far as the queue cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    Idle,
    Preparing,
    Running,
    Finished,
    Failed,
}

impl Machine {
    /// What the machine's own `gcode_state` means to the queue.
    ///
    /// Bambu reports a state name rather than an enum: `IDLE`, `PREPARE`,
    /// `RUNNING`, `PAUSE`, `FINISH`, `FAILED`. Anything else — a state a
    /// firmware adds later, or a report that carries none — is `None`, and the
    /// caller must not turn a `None` into a guess: an unknown state is not a
    /// failure, and it is certainly not a licence to start the next job.
    pub fn from_report(state: &str) -> Option<Machine> {
        match state.trim().to_ascii_uppercase().as_str() {
            "IDLE" => Some(Machine::Idle),
            "PREPARE" | "PREPARING" | "SLICING" => Some(Machine::Preparing),
            "RUNNING" | "PAUSE" | "PAUSED" => Some(Machine::Running),
            "FINISH" | "FINISHED" => Some(Machine::Finished),
            "FAILED" | "FAIL" => Some(Machine::Failed),
            _ => None,
        }
    }
}

/// Where the run has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    /// Nothing authorised, nothing in flight.
    Idle,
    /// A press authorised the list; one job may go out.
    Armed,
    /// A job is out on the machine.
    Printing,
    /// A job finished. The bed holds it, and nothing moves until a person
    /// says the plate is clear.
    WaitingForPlate,
    /// Experimental removal: the print finished and the bed is still hot.
    Cooling,
    /// Experimental removal: the bed is cool and the sweep lines may be sent.
    Sweeping,
    /// Experimental removal: compare the camera with the empty plate.
    CheckingPlate,
    /// Experimental removal: the camera says the plate is empty, so one more
    /// job may go. This is not the same as [`RunState::Armed`], which is a press.
    Ready,
    /// Over: finished, cleared, failed, or stopped by the operator.
    Stopped,
}

/// The experimental A1 removal path. Off unless a person turns it on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EjectMode {
    #[default]
    Off,
    /// The sweep is already at the end of the queued file.
    InFile,
    /// The sweep is sent as lines after the bed has cooled.
    AfterPrint,
}

/// What an observation changed, and what to tell the operator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub state: RunState,
    pub note: Option<String>,
}

impl Update {
    fn of(state: RunState) -> Self {
        Self { state, note: None }
    }

    fn with(state: RunState, note: impl Into<String>) -> Self {
        Self {
            state,
            note: Some(note.into()),
        }
    }
}

#[derive(Debug, Default)]
pub struct QueueRun {
    state: Option<RunState>,
    /// Whether a job has been seen running since the last start. A `FINISH`
    /// that arrives without it is a stale report from the machine, not the end
    /// of the job this queue sent.
    seen_running: bool,
    eject: EjectMode,
}

impl QueueRun {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> RunState {
        self.state.unwrap_or(RunState::Idle)
    }

    /// The operator pressed start. This is the one press that authorises a
    /// list, and it authorises exactly one job at a time.
    pub fn arm(&mut self) -> Update {
        self.seen_running = false;
        self.state = Some(RunState::Armed);
        Update::of(RunState::Armed)
    }

    /// Whether a job may be sent right now. A fresh press, a plate cleared by
    /// hand, or — only on the experimental path — a camera that says the plate
    /// is empty.
    pub fn may_start(&self) -> bool {
        matches!(self.state(), RunState::Armed | RunState::Ready)
    }

    pub fn eject(&self) -> EjectMode {
        self.eject
    }

    /// Turn the experimental path on or off. This does not move a machine.
    pub fn set_eject(&mut self, mode: EjectMode) -> Update {
        self.eject = mode;
        Update::of(self.state())
    }

    /// The sweep lines may be published. Only the after-print path, and only
    /// once the bed has cooled.
    pub fn may_send_sweep(&self) -> bool {
        self.state() == RunState::Sweeping && self.eject == EjectMode::AfterPrint
    }

    /// The job went out.
    pub fn started(&mut self) -> Update {
        self.seen_running = false;
        self.state = Some(RunState::Printing);
        Update::of(RunState::Printing)
    }

    /// A report from the machine. Never starts anything.
    pub fn observe(&mut self, machine: Machine) -> Update {
        match machine {
            Machine::Preparing | Machine::Running => {
                if self.state() == RunState::Printing {
                    self.seen_running = true;
                }
                Update::of(self.state())
            }
            Machine::Failed => {
                if matches!(
                    self.state(),
                    RunState::Armed | RunState::Printing | RunState::WaitingForPlate
                ) {
                    self.state = Some(RunState::Stopped);
                    return Update::with(
                        RunState::Stopped,
                        "queue stopped — the machine reported a failed print",
                    );
                }
                Update::of(self.state())
            }
            Machine::Finished | Machine::Idle => {
                if self.state() == RunState::Printing && self.seen_running {
                    return self.finish();
                }
                Update::of(self.state())
            }
        }
    }

    fn finish(&mut self) -> Update {
        match self.eject {
            EjectMode::Off => {
                self.state = Some(RunState::WaitingForPlate);
                Update::with(
                    RunState::WaitingForPlate,
                    "print finished — clear the plate, then start the next job",
                )
            }
            EjectMode::InFile => {
                self.state = Some(RunState::CheckingPlate);
                Update::with(
                    RunState::CheckingPlate,
                    "print finished — the sweep was in the file, checking the plate with the camera",
                )
            }
            EjectMode::AfterPrint => {
                self.state = Some(RunState::Cooling);
                Update::with(
                    RunState::Cooling,
                    "print finished — waiting for the bed to cool before the sweep",
                )
            }
        }
    }

    /// A bed-temperature report. Motion stays forbidden until the bed is at or
    /// below `target_c`.
    pub fn cooled(&mut self, bed_c: f64, target_c: f64) -> Update {
        if self.state() != RunState::Cooling {
            return Update::of(self.state());
        }
        if bed_c > target_c {
            return Update::with(
                RunState::Cooling,
                format!("bed is {bed_c:.0} °C — waiting until it reaches {target_c:.0} °C"),
            );
        }
        self.state = Some(RunState::Sweeping);
        Update::with(
            RunState::Sweeping,
            format!("bed is {bed_c:.0} °C — the sweep may be sent"),
        )
    }

    /// The sweep lines have been handed to the link. The plate is checked next.
    pub fn sweep_sent(&mut self) -> Update {
        if self.state() == RunState::Sweeping {
            self.state = Some(RunState::CheckingPlate);
            return Update::with(
                RunState::CheckingPlate,
                "sweep sent — checking the plate with the camera",
            );
        }
        Update::of(self.state())
    }

    /// The camera's comparison. A clear plate allows exactly one more job. An
    /// occupied plate stops the run.
    pub fn plate_verdict(&mut self, clear: bool) -> Update {
        if self.state() != RunState::CheckingPlate {
            return Update::of(self.state());
        }
        if clear {
            self.state = Some(RunState::Ready);
            Update::with(
                RunState::Ready,
                "the plate matches the empty-plate picture — the next job may go",
            )
        } else {
            self.state = Some(RunState::Stopped);
            self.seen_running = false;
            Update::with(
                RunState::Stopped,
                "the plate is not clear — the queue has stopped",
            )
        }
    }

    /// The operator says the plate is clear. That is the only way a run goes
    /// back to being allowed to send a job.
    pub fn plate_cleared(&mut self) -> Update {
        if self.state() == RunState::WaitingForPlate {
            self.state = Some(RunState::Armed);
            return Update::with(RunState::Armed, "plate clear — the next job may go");
        }
        Update::of(self.state())
    }

    /// Something went wrong on our side, or the operator stopped it.
    pub fn stop(&mut self, why: &str) -> Update {
        self.state = Some(RunState::Stopped);
        self.seen_running = false;
        Update::with(RunState::Stopped, why)
    }

    /// Nothing left to print.
    pub fn drained(&mut self) -> Update {
        self.state = Some(RunState::Stopped);
        self.seen_running = false;
        Update::with(RunState::Stopped, "queue done — nothing left to print")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printing() -> QueueRun {
        let mut run = QueueRun::new();
        run.arm();
        run.may_start();
        run.started();
        run
    }

    #[test]
    fn a_finished_print_waits_for_the_plate_rather_than_starting_the_next() {
        let mut run = printing();
        run.observe(Machine::Running);
        let update = run.observe(Machine::Finished);
        assert_eq!(update.state, RunState::WaitingForPlate);
        assert!(
            update.note.is_some(),
            "the operator has to be told what to do"
        );
        assert!(
            !run.may_start(),
            "nothing may go out until the plate is cleared"
        );
    }

    #[test]
    fn an_idle_machine_never_starts_a_job_by_itself() {
        let mut run = printing();
        run.observe(Machine::Running);
        run.observe(Machine::Finished);
        for machine in [Machine::Idle, Machine::Preparing, Machine::Finished] {
            let update = run.observe(machine);
            assert_ne!(update.state, RunState::Armed, "{machine:?} armed the queue");
            assert!(!run.may_start(), "{machine:?} allowed a job out");
        }
    }

    #[test]
    fn a_stale_finish_without_a_print_does_not_end_the_job() {
        // The machine can still be reporting the previous print when a new job
        // is sent. That FINISH is not the end of this one.
        let mut run = printing();
        let update = run.observe(Machine::Finished);
        assert_eq!(update.state, RunState::Printing);
        assert!(!run.may_start());
    }

    #[test]
    fn a_failed_print_stops_the_run() {
        let mut run = printing();
        run.observe(Machine::Running);
        let update = run.observe(Machine::Failed);
        assert_eq!(update.state, RunState::Stopped);
        assert!(update.note.unwrap().contains("failed"));
        assert!(!run.may_start());
        // And a later idle cannot revive it.
        assert_eq!(run.observe(Machine::Idle).state, RunState::Stopped);
    }

    #[test]
    fn clearing_the_plate_arms_exactly_one_more_job() {
        let mut run = printing();
        run.observe(Machine::Running);
        run.observe(Machine::Finished);
        assert_eq!(run.plate_cleared().state, RunState::Armed);
        assert!(
            run.may_start(),
            "clearing the plate is what allows the next job"
        );
        run.started();
        assert!(!run.may_start(), "one job, not two");
    }

    #[test]
    fn an_operator_stop_or_an_error_ends_the_run() {
        for why in ["stopped by hand", "the host refused the job"] {
            let mut run = printing();
            assert_eq!(run.stop(why).state, RunState::Stopped);
            assert!(!run.may_start());
        }
    }

    #[test]
    fn a_drained_queue_is_stopped_not_armed() {
        let mut run = printing();
        let update = run.drained();
        assert_eq!(update.state, RunState::Stopped);
        assert!(update.note.unwrap().contains("nothing left"));
        assert!(!run.may_start());
    }

    #[test]
    fn the_machine_s_state_names_map_onto_the_queue_s() {
        assert_eq!(Machine::from_report("RUNNING"), Some(Machine::Running));
        assert_eq!(Machine::from_report("pause"), Some(Machine::Running));
        assert_eq!(Machine::from_report(" PREPARE "), Some(Machine::Preparing));
        assert_eq!(Machine::from_report("FINISH"), Some(Machine::Finished));
        assert_eq!(Machine::from_report("FAILED"), Some(Machine::Failed));
        assert_eq!(Machine::from_report("IDLE"), Some(Machine::Idle));
        // A state this build has never heard of is not an event.
        assert_eq!(Machine::from_report("HEATING"), None);
        assert_eq!(Machine::from_report(""), None);
    }

    #[test]
    fn removal_stays_off_and_a_finish_still_waits_for_a_person() {
        let mut run = printing();
        assert_eq!(run.eject(), EjectMode::Off);
        assert!(!run.may_send_sweep());
        run.observe(Machine::Running);
        assert_eq!(
            run.observe(Machine::Finished).state,
            RunState::WaitingForPlate
        );
        assert!(!run.may_start());
    }

    #[test]
    fn an_in_file_sweep_checks_the_plate_and_only_then_allows_the_next_job() {
        let mut run = printing();
        run.set_eject(EjectMode::InFile);
        run.observe(Machine::Running);
        assert_eq!(
            run.observe(Machine::Finished).state,
            RunState::CheckingPlate
        );
        assert!(!run.may_start());
        assert!(!run.may_send_sweep());
        assert_eq!(run.plate_verdict(false).state, RunState::Stopped);
        assert!(!run.may_start());

        let mut run = printing();
        run.set_eject(EjectMode::InFile);
        run.observe(Machine::Running);
        run.observe(Machine::Finished);
        assert_eq!(run.plate_verdict(true).state, RunState::Ready);
        assert!(run.may_start());
        run.started();
        assert!(!run.may_start(), "the clear plate authorises one job");
    }

    #[test]
    fn an_after_print_sweep_waits_for_the_bed_before_any_line_may_be_sent() {
        let mut run = printing();
        run.set_eject(EjectMode::AfterPrint);
        run.observe(Machine::Running);
        assert_eq!(run.observe(Machine::Finished).state, RunState::Cooling);
        assert!(!run.may_send_sweep());
        let waiting = run.cooled(40.0, 28.0);
        assert_eq!(waiting.state, RunState::Cooling);
        assert!(waiting.note.unwrap().contains("40"));
        assert!(!run.may_send_sweep());
        assert_eq!(run.cooled(27.5, 28.0).state, RunState::Sweeping);
        assert!(run.may_send_sweep());
        assert!(!run.may_start());
        assert_eq!(run.sweep_sent().state, RunState::CheckingPlate);
        assert!(!run.may_send_sweep(), "the lines are sent once");
    }

    #[test]
    fn a_fresh_press_can_start_again_after_a_stop() {
        let mut run = printing();
        run.observe(Machine::Running);
        run.observe(Machine::Failed);
        assert_eq!(run.state(), RunState::Stopped);
        assert_eq!(run.arm().state, RunState::Armed);
        assert!(
            run.may_start(),
            "a person pressed start; that is authorisation"
        );
    }
}
