//! Camera frames and the experimental A1 sweep, as commands the window calls.
//!
//! A frame never moves a printer. The sweep lines are sent only by
//! `removal_send`, and only when the queue is in the state that allows it.

use std::time::Duration;

use serde::Serialize;
use shale_core::removal::{self, MachineBed, PartBox, SweepDecision, SweepRequest};
use shale_host::camera::{self, CameraTarget, Sensitivity, WatchSettings};
use tauri::{AppHandle, State};

use crate::{config_dir, lock, AppState, Inner, PrinterHost};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraView {
    pub running: bool,
    pub path: String,
    pub frames: u64,
    pub fps: f64,
    pub has_jpeg: bool,
    pub codec: String,
    pub fault_code: Option<String>,
    pub fault: Option<String>,
}

fn view_of(status: camera::CameraStatus) -> CameraView {
    CameraView {
        running: status.running,
        path: status.path,
        frames: status.frames,
        fps: status.fps,
        has_jpeg: status.has_jpeg,
        codec: status.codec,
        fault_code: status.fault_code,
        fault: status.fault,
    }
}

fn stopped() -> CameraView {
    CameraView {
        running: false,
        path: String::new(),
        frames: 0,
        fps: 0.0,
        has_jpeg: false,
        codec: String::new(),
        fault_code: None,
        fault: None,
    }
}

fn target(host: String, model: String, code: String) -> Result<CameraTarget, String> {
    if host.trim().is_empty() || code.trim().is_empty() {
        return Err("A host and the printer's access code are both needed.".into());
    }
    Ok(CameraTarget::new(host, &model, code))
}

fn camera_slot(
    host: &PrinterHost,
) -> Result<std::sync::MutexGuard<'_, Option<camera::CameraSession>>, String> {
    host.camera
        .lock()
        .map_err(|_| "the camera lock is poisoned".to_string())
}

#[tauri::command]
pub fn camera_start(
    printer: State<PrinterHost>,
    host: String,
    model: String,
    code: String,
) -> Result<CameraView, String> {
    let target = target(host, model, code)?;
    let mut slot = camera_slot(&printer)?;
    if let Some(old) = slot.take() {
        old.stop();
    }
    let session = camera::CameraSession::start(target);
    let status = session.status();
    *slot = Some(session);
    Ok(view_of(status))
}

#[tauri::command]
pub fn camera_stop(printer: State<PrinterHost>) -> Result<CameraView, String> {
    let mut slot = camera_slot(&printer)?;
    if let Some(old) = slot.take() {
        old.stop();
    }
    Ok(stopped())
}

#[tauri::command]
pub fn camera_status(printer: State<PrinterHost>) -> Result<CameraView, String> {
    let slot = camera_slot(&printer)?;
    Ok(match slot.as_ref() {
        Some(session) => view_of(session.status()),
        None => stopped(),
    })
}

#[tauri::command]
pub fn camera_frame(printer: State<PrinterHost>) -> Result<Option<String>, String> {
    let slot = camera_slot(&printer)?;
    Ok(slot
        .as_ref()
        .and_then(|session| session.latest_jpeg())
        .map(|bytes| camera::base64(&bytes)))
}

/// One still. Uses the frame already in hand when the stream is running, and
/// otherwise opens the socket just long enough for a single picture.
#[tauri::command]
pub fn camera_snapshot(
    printer: State<PrinterHost>,
    host: String,
    model: String,
    code: String,
) -> Result<String, String> {
    if let Some(jpeg) = camera_frame(printer)? {
        return Ok(jpeg);
    }
    let target = target(host, model, code)?;
    let jpeg =
        camera::snapshot_jpeg(&target, Duration::from_secs(8)).map_err(|err| err.message())?;
    Ok(camera::base64(&jpeg))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchView {
    pub kind: String,
    pub confidence: f64,
    pub alert: bool,
    pub action: String,
    pub boxes: Vec<WatchBox>,
    pub heatmap: Vec<f64>,
    pub heatmap_cols: usize,
    pub heatmap_rows: usize,
    pub note: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub label: String,
    pub confidence: f64,
}

fn quiet() -> WatchView {
    WatchView {
        kind: "none".into(),
        confidence: 0.0,
        alert: false,
        action: "none".into(),
        boxes: Vec::new(),
        heatmap: Vec::new(),
        heatmap_cols: 0,
        heatmap_rows: 0,
        note: "No picture yet.".into(),
    }
}

/// Judge the latest frame. Pause is reported, never sent: the window sends
/// pause only when this printer was opted in.
#[tauri::command]
pub fn camera_watch(
    printer: State<PrinterHost>,
    confidence: f64,
    sensitivity: String,
    debounce_frames: u32,
    pause: bool,
) -> Result<WatchView, String> {
    let jpeg = {
        let slot = camera_slot(&printer)?;
        match slot.as_ref().and_then(|session| session.latest_jpeg()) {
            Some(jpeg) => jpeg,
            None => return Ok(quiet()),
        }
    };
    let current = camera::decode_jpeg(&jpeg)?;
    let mut previous_bytes = printer
        .watch_previous
        .lock()
        .map_err(|_| "the camera lock is poisoned".to_string())?;
    let before = previous_bytes
        .as_ref()
        .and_then(|bytes| camera::decode_jpeg(bytes).ok());
    let settings = WatchSettings {
        sample_seconds: 5.0,
        confidence,
        sensitivity: Sensitivity::parse(&sensitivity),
        debounce_frames,
        pause,
    };
    let finding = camera::inspect(&current, before.as_ref(), &settings);
    *previous_bytes = Some(jpeg);
    drop(previous_bytes);
    let mut watch = printer
        .watch
        .lock()
        .map_err(|_| "the camera lock is poisoned".to_string())?;
    watch.set_need(debounce_frames.max(1));
    let alert = watch.push(&finding, confidence);
    let action = camera::watch_action(alert, pause);
    Ok(WatchView {
        kind: finding.kind.as_str().into(),
        confidence: finding.confidence,
        alert,
        action: action.into(),
        boxes: finding
            .boxes
            .into_iter()
            .map(|mark| WatchBox {
                x: mark.x,
                y: mark.y,
                w: mark.w,
                h: mark.h,
                label: mark.label,
                confidence: mark.confidence,
            })
            .collect(),
        heatmap: finding.heatmap,
        heatmap_cols: finding.heatmap_cols,
        heatmap_rows: finding.heatmap_rows,
        note: finding.note,
    })
}

fn plate_file(app: &AppHandle, serial: &str) -> Result<std::path::PathBuf, String> {
    let safe: String = serial
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let name = if safe.is_empty() {
        "printer".to_string()
    } else {
        safe
    };
    let dir = config_dir(app)?.join("camera-plates");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir.join(format!("{name}.jpg")))
}

#[tauri::command]
pub fn camera_save_reference(
    app: AppHandle,
    printer: State<PrinterHost>,
    serial: String,
) -> Result<(), String> {
    let slot = camera_slot(&printer)?;
    let jpeg = slot
        .as_ref()
        .and_then(|session| session.latest_jpeg())
        .ok_or_else(|| "There is no picture to keep. Start the camera first.".to_string())?;
    let path = plate_file(&app, &serial)?;
    std::fs::write(&path, jpeg).map_err(|err| err.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlateView {
    pub clear: bool,
    pub difference: f64,
    pub note: String,
}

#[tauri::command]
pub fn camera_compare(
    app: AppHandle,
    printer: State<PrinterHost>,
    serial: String,
) -> Result<PlateView, String> {
    let path = plate_file(&app, &serial)?;
    let reference = std::fs::read(&path).map_err(|_| {
        "Take a picture of the empty plate first. The comparison has nothing to go on.".to_string()
    })?;
    let slot = camera_slot(&printer)?;
    let jpeg = slot
        .as_ref()
        .and_then(|session| session.latest_jpeg())
        .ok_or_else(|| "There is no live picture to compare.".to_string())?;
    let reference = camera::decode_jpeg(&reference)?;
    let current = camera::decode_jpeg(&jpeg)?;
    let verdict = camera::plates_match(&reference, &current, 12.0);
    Ok(PlateView {
        clear: verdict.clear,
        difference: verdict.difference,
        note: verdict.note,
    })
}

pub(crate) struct SweepPlan {
    pub accepted: bool,
    pub reasons: Vec<String>,
    pub gcode: String,
    pub lines: Vec<String>,
}

pub(crate) fn plan_sweep(
    inner: &Inner,
    printer_id: &str,
    cool_c: f64,
    center_is_zero: bool,
    bed_width: f64,
    bed_depth: f64,
    bed_height: f64,
    adhesion: &str,
) -> Result<SweepPlan, String> {
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    let mut any = false;
    for model in inner.models.iter().filter(|model| model.visible) {
        let baked = shale_core::mesh::bake(&model.mesh, &model.transform);
        let (lo, hi) = shale_core::mesh::bounds(&baked);
        any = true;
        for axis in 0..3 {
            min[axis] = min[axis].min(lo[axis]);
            max[axis] = max[axis].max(hi[axis]);
        }
    }
    if !any {
        return Err("There is no part on the plate.".into());
    }
    let bed = if center_is_zero {
        MachineBed::centred(bed_width, bed_depth, bed_height)
    } else {
        MachineBed {
            x_min: 0.0,
            y_min: 0.0,
            width: bed_width,
            depth: bed_depth,
            z_max: bed_height,
            bed_slinger: true,
        }
    };
    let request = SweepRequest {
        printer_id: printer_id.to_string(),
        bed,
        part: PartBox {
            min_x: min[0],
            max_x: max[0],
            min_y: min[1],
            max_y: max[1],
            min_z: min[2],
            max_z: max[2],
        },
        adhesion: adhesion.to_string(),
        cool_c,
        allow_other_machines: false,
    };
    Ok(match removal::sweep(&request) {
        SweepDecision::Refuse { reasons } => SweepPlan {
            accepted: false,
            reasons,
            gcode: String::new(),
            lines: Vec::new(),
        },
        SweepDecision::Accept { gcode, lines } => SweepPlan {
            accepted: true,
            reasons: Vec::new(),
            gcode,
            lines,
        },
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalView {
    pub offered: bool,
    pub accepted: bool,
    pub reasons: Vec<String>,
    pub gcode: String,
}

#[tauri::command]
pub fn removal_preview(
    state: State<AppState>,
    printer_id: String,
    cool_c: f64,
    center_is_zero: bool,
    bed_width: f64,
    bed_depth: f64,
    bed_height: f64,
    adhesion: String,
) -> Result<RemovalView, String> {
    let inner = lock(&state)?;
    if !removal::offered_for(&printer_id) {
        return Ok(RemovalView {
            offered: false,
            accepted: false,
            reasons: vec![format!(
                "Automatic removal is only offered for the A1. This machine is {printer_id}."
            )],
            gcode: String::new(),
        });
    }
    let plan = plan_sweep(
        &inner,
        &printer_id,
        cool_c,
        center_is_zero,
        bed_width,
        bed_depth,
        bed_height,
        &adhesion,
    )?;
    Ok(RemovalView {
        offered: true,
        accepted: plan.accepted,
        reasons: plan.reasons,
        gcode: plan.gcode,
    })
}
