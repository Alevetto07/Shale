//! Read-only printer status.
//!
//! The cloud device list and the LAN status report are the two shapes Shale
//! already understands. Neither one starts, pauses, or stops a print. This
//! process has no credential vault, so the account it can see is empty and
//! every call says to sign in in the desktop.

use std::sync::Arc;

use serde::Serialize;
use shale_host::cloud::{api, Cloud, CloudDevice, HttpTransport, MemoryStore};
use shale_host::Status;

pub const SIGN_IN: &str = "sign in to Bambu in Shale first";

#[derive(Clone, Debug, Serialize)]
pub struct AmsSlot {
    pub index: usize,
    pub material: String,
    pub colour: String,
    pub humidity_percent: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PrintJob {
    pub id: String,
    pub name: String,
    pub state: String,
    pub progress_percent: Option<f64>,
    /// Minutes left, when the printer reported them.
    pub remaining_minutes: Option<f64>,
    pub file: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PrinterView {
    pub id: String,
    pub name: String,
    pub model: String,
    pub online: bool,
    pub state: String,
    pub job: Option<PrintJob>,
    pub nozzle_c: Option<f64>,
    pub nozzle_target_c: Option<f64>,
    pub bed_c: Option<f64>,
    pub bed_target_c: Option<f64>,
    pub ams: Vec<AmsSlot>,
    pub note: String,
}

pub trait PrinterDirectory: Send + Sync {
    fn printers(&self) -> Result<Vec<PrinterView>, String>;
    fn status(&self, id: &str) -> Result<PrinterView, String>;
    fn jobs(&self) -> Result<Vec<PrintJob>, String>;
}

/// The Bambu account this process can see. There is no keyring here, so the
/// store is empty and the tools ask for a sign-in.
pub struct AccountPrinters;

impl PrinterDirectory for AccountPrinters {
    fn printers(&self) -> Result<Vec<PrinterView>, String> {
        let mut cloud = Cloud::new(
            api::GLOBAL_HOST,
            Arc::new(HttpTransport),
            Arc::new(MemoryStore::default()),
        );
        cloud.load();
        if !cloud.signed_in() {
            return Err(SIGN_IN.to_string());
        }
        cloud
            .devices()
            .map(|list| list.into_iter().map(|device| from_cloud(&device)).collect())
            .map_err(|err| err.to_string())
    }

    fn status(&self, id: &str) -> Result<PrinterView, String> {
        self.printers()?
            .into_iter()
            .find(|printer| printer.id == id)
            .ok_or_else(|| format!("no printer '{id}'"))
    }

    fn jobs(&self) -> Result<Vec<PrintJob>, String> {
        Ok(self
            .printers()?
            .into_iter()
            .filter_map(|printer| printer.job)
            .collect())
    }
}

pub fn from_cloud(device: &CloudDevice) -> PrinterView {
    let state = device.print_status.clone().unwrap_or_else(|| {
        if device.online {
            "IDLE".into()
        } else {
            "OFFLINE".into()
        }
    });
    let job = device.task_id.as_ref().and_then(|id| {
        if id.is_empty() {
            return None;
        }
        Some(PrintJob {
            id: id.clone(),
            name: String::new(),
            state: state.clone(),
            progress_percent: device.mc_percent.map(|value| value as f64),
            remaining_minutes: device.mc_remaining_time.map(|value| value as f64),
            file: String::new(),
        })
    });
    PrinterView {
        id: device.dev_id.clone(),
        name: device.label(),
        model: if device.dev_model_name.trim().is_empty() {
            device.dev_product_name.clone()
        } else {
            device.dev_model_name.clone()
        },
        online: device.online,
        state,
        job,
        nozzle_c: None,
        nozzle_target_c: None,
        bed_c: None,
        bed_target_c: None,
        ams: Vec::new(),
        note: "Temperatures and AMS humidity are not in the cloud device list.".into(),
    }
}

/// A LAN report fills temperatures the cloud list does not have. Nothing in
/// this process opens a LAN socket; a directory that has a report can use this.
#[allow(dead_code)]
pub fn from_lan(id: &str, name: &str, status: &Status) -> PrinterView {
    let busy = status.is_printing() || !status.file.is_empty();
    PrinterView {
        id: id.to_string(),
        name: name.to_string(),
        model: String::new(),
        online: status.connected,
        state: if status.state.is_empty() {
            "IDLE".into()
        } else {
            status.state.clone()
        },
        job: busy.then(|| PrintJob {
            id: String::new(),
            name: status.file.clone(),
            state: status.state.clone(),
            progress_percent: Some(status.percent as f64),
            remaining_minutes: Some(status.remaining_min as f64),
            file: status.file.clone(),
        }),
        nozzle_c: Some(status.nozzle),
        nozzle_target_c: Some(status.nozzle_target),
        bed_c: Some(status.bed),
        bed_target_c: Some(status.bed_target),
        ams: Vec::new(),
        note: "AMS humidity is not in the LAN status report.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cloud_printer_keeps_its_job_and_leaves_the_ams_empty() {
        let device = CloudDevice {
            dev_id: "01".into(),
            name: "Shed".into(),
            dev_model_name: "N2S".into(),
            online: true,
            print_status: Some("RUNNING".into()),
            task_id: Some("task-9".into()),
            mc_percent: Some(40),
            mc_remaining_time: Some(12),
            ..CloudDevice::default()
        };
        let view = from_cloud(&device);
        assert_eq!(view.name, "Shed");
        assert_eq!(view.job.as_ref().map(|job| job.progress_percent), Some(Some(40.0)));
        assert!(view.ams.is_empty());
        assert!(view.nozzle_c.is_none());
    }

    #[test]
    fn a_lan_report_fills_the_temperatures() {
        let status = Status {
            connected: true,
            state: "RUNNING".into(),
            percent: 10,
            remaining_min: 5,
            nozzle: 210.0,
            nozzle_target: 220.0,
            bed: 55.0,
            bed_target: 60.0,
            file: "cube.gcode".into(),
            ..Status::default()
        };
        let view = from_lan("lan", "Bench", &status);
        assert_eq!(view.nozzle_c, Some(210.0));
        assert_eq!(view.bed_target_c, Some(60.0));
        assert!(view.ams.is_empty());
        assert_eq!(view.job.as_ref().map(|job| job.file.as_str()), Some("cube.gcode"));
    }
}
