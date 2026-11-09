//! What an overnight queue is allowed to do on its own — which is nothing.
//!
//! A queue is a list of jobs and one press that authorises the list. The press
//! covers the first job; every job after it needs a second, explicit press,
//! because when a print finishes the bed still holds the part that was just
//! printed. A queue that advances itself prints into that part, then into the
//! next one, and the operator finds three failed prints instead of one.
//!
//! The state machine lives here, away from the window, so the rule can be
//! tested without a printer: [`QueueRun::observe`] never returns anything that
//! starts a job — structurally, not by a check somebody can forget.

/// What the machine is doing, as far as the queue cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    Idle,
    Preparing,
    Running,
    Finished,
    Failed,
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
    /// Over: finished, cleared, failed, or stopped by the operator.
    Stopped,
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

    /// Whether a job may be sent right now. Only a fresh press or a plate
    /// cleared by hand puts the run here.
    pub fn may_start(&self) -> bool {
        self.state() == RunState::Armed
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
                    self.state = Some(RunState::WaitingForPlate);
                    return Update::with(
                        RunState::WaitingForPlate,
                        "print finished — clear the plate, then start the next job",
                    );
                }
                Update::of(self.state())
            }
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
        assert!(update.note.is_some(), "the operator has to be told what to do");
        assert!(!run.may_start(), "nothing may go out until the plate is cleared");
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
        assert!(run.may_start(), "clearing the plate is what allows the next job");
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
    fn a_fresh_press_can_start_again_after_a_stop() {
        let mut run = printing();
        run.observe(Machine::Running);
        run.observe(Machine::Failed);
        assert_eq!(run.state(), RunState::Stopped);
        assert_eq!(run.arm().state, RunState::Armed);
        assert!(run.may_start(), "a person pressed start; that is authorisation");
    }
}
