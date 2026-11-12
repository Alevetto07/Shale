//! Reading the device list the way a person would.
//!
//! [`super::api::CloudDevice`] is Bambu's shape, with every field optional
//! because an offline machine arrives with almost none of them. This turns one
//! into a line for the window and a state a decision can be made on.

use super::api::CloudDevice;

/// What a printer is doing, as far as the cloud knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The cloud has no connection to the machine.
    Offline,
    /// Connected and idle.
    Idle,
    /// Connected and printing; `percent` is 0-100 when the cloud says.
    Printing { percent: Option<i64> },
    /// Connected but not in a state a task can be sent to.
    Busy,
}

impl State {
    pub fn of(device: &CloudDevice) -> Self {
        if !device.online {
            return Self::Offline;
        }
        match device.print_status.as_deref() {
            Some("RUNNING") | Some("PREPARE") | Some("PAUSE") | Some("SLICING") => Self::Printing {
                percent: device.mc_percent,
            },
            Some("IDLE") | Some("FINISH") | Some("FAILED") => Self::Idle,
            Some(_) => Self::Busy,
            None => match device.mc_print_stage.as_deref() {
                Some("0") | None => Self::Idle,
                Some(_) => Self::Busy,
            },
        }
    }

    /// Whether a print may be sent. An offline machine is not a refusal by
    /// Shale: the cloud would queue the task and the person would not know why
    /// nothing happened.
    pub fn may_print(self) -> bool {
        matches!(self, Self::Idle)
    }

    pub fn line(self) -> String {
        match self {
            Self::Offline => "offline".into(),
            Self::Idle => "idle".into(),
            Self::Printing {
                percent: Some(percent),
            } => format!("printing, {percent}%"),
            Self::Printing { percent: None } => "printing".into(),
            Self::Busy => "busy".into(),
        }
    }
}

/// One row for the window: what to call it, what it is doing, and whether it
/// can take a print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub dev_id: String,
    pub label: String,
    pub model: String,
    pub state: State,
}

pub fn rows(devices: &[CloudDevice]) -> Vec<Row> {
    let mut rows: Vec<Row> = devices
        .iter()
        .map(|device| Row {
            dev_id: device.dev_id.clone(),
            label: device.label(),
            model: if device.dev_model_name.is_empty() {
                device.dev_product_name.clone()
            } else {
                device.dev_model_name.clone()
            },
            state: State::of(device),
        })
        .collect();
    // Signed-in machines first, then by name: the one that can print is the one
    // the eye should find first.
    rows.sort_by(|left, right| {
        right
            .state
            .may_print()
            .cmp(&left.state.may_print())
            .then_with(|| left.label.cmp(&right.label))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(online: bool, status: Option<&str>) -> CloudDevice {
        CloudDevice {
            dev_id: "01".into(),
            name: "workshop".into(),
            online,
            print_status: status.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn an_offline_machine_cannot_take_a_print() {
        let state = State::of(&device(false, Some("IDLE")));
        assert_eq!(state, State::Offline);
        assert!(!state.may_print());
    }

    #[test]
    fn an_idle_machine_can() {
        assert!(State::of(&device(true, Some("IDLE"))).may_print());
    }

    #[test]
    fn a_printing_machine_says_how_far() {
        let mut device = device(true, Some("RUNNING"));
        device.mc_percent = Some(42);
        assert_eq!(State::of(&device).line(), "printing, 42%");
        assert!(!State::of(&device).may_print());
    }

    #[test]
    fn the_list_puts_the_machines_that_can_print_first() {
        let mut busy = device(true, Some("RUNNING"));
        busy.name = "alpha".into();
        let mut free = device(true, Some("IDLE"));
        free.name = "zulu".into();
        let rows = rows(&[busy, free]);
        assert_eq!(rows[0].label, "zulu");
        assert_eq!(rows[1].label, "alpha");
    }
}
