//! Camera frames and the experimental A1 sweep, as commands the window calls.
//!
//! A frame never moves a printer. The sweep lines are sent only by
//! `removal_send`, and only when the queue is in the state that allows it.

use std::sync::Mutex;
use std::thread;
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
    app: AppHandle,
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
    let library = codec_library(&app);
    let session = camera::CameraSession::start_with(target, library);
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
    app: AppHandle,
    printer: State<PrinterHost>,
    host: String,
    model: String,
    code: String,
) -> Result<String, String> {
    if let Some(jpeg) = camera_frame(printer)? {
        return Ok(jpeg);
    }
    let target = target(host, model, code)?;
    let library = codec_library(&app);
    let jpeg = camera::snapshot_jpeg_with(&target, Duration::from_secs(8), library.as_deref())
        .map_err(|err| err.message())?;
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
    pub detector: String,
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
        detector: "builtin".into(),
    }
}

/// Judge the latest frame. Pause is reported, never sent: the window sends
/// pause only when this printer was opted in.
#[tauri::command]
pub fn camera_watch(
    app: AppHandle,
    printer: State<PrinterHost>,
    confidence: f64,
    sensitivity: String,
    debounce_frames: u32,
    pause: bool,
    detector: Option<String>,
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
    let want_model = detector.as_deref() == Some("obico");
    let model_file = config_dir(&app)?.join("failure-models").join(camera::MODEL_FILE);
    let (finding, detector_name) = if want_model {
        match camera::score_obico(&model_file, &current, &settings) {
            Ok(finding) => (finding, "obico".to_string()),
            Err(reason) => {
                let mut finding = camera::inspect(&current, before.as_ref(), &settings);
                finding.note = format!("{reason} Using the built-in detector.");
                (finding, "builtin".to_string())
            }
        }
    } else {
        (
            camera::inspect(&current, before.as_ref(), &settings),
            "builtin".to_string(),
        )
    };
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
        detector: detector_name,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub phase: String,
    pub ready: bool,
    pub received: u64,
    pub total: u64,
    pub message: String,
}

struct ModelProgress {
    phase: String,
    received: u64,
    total: u64,
    message: String,
}

fn model_progress() -> &'static Mutex<ModelProgress> {
    static PROGRESS: Mutex<ModelProgress> = Mutex::new(ModelProgress {
        phase: String::new(),
        received: 0,
        total: 0,
        message: String::new(),
    });
    &PROGRESS
}

fn codec_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    // `SHALE_CACHE_DIR` wins, so a test never writes the real app data folder.
    // The desktop app leaves that unset and lands in the same config directory.
    let _ = app;
    Ok(camera::data_root().join("openh264"))
}

fn codec_library(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = codec_dir(app).ok()?;
    camera::installed_library(&dir)
}

fn model_file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(config_dir(app)?.join("failure-models").join(camera::MODEL_FILE))
}

fn status_of(path: &std::path::Path) -> ModelStatus {
    if camera::model_is_verified(path) {
        return ModelStatus {
            phase: "ready".into(),
            ready: true,
            received: camera::OBICO_ONNX_BYTES,
            total: camera::OBICO_ONNX_BYTES,
            message: "The failure model is ready.".into(),
        };
    }
    let progress = model_progress().lock().unwrap();
    let phase = if progress.phase.is_empty() {
        "missing"
    } else {
        progress.phase.as_str()
    };
    ModelStatus {
        phase: phase.into(),
        ready: false,
        received: progress.received,
        total: if progress.total == 0 {
            camera::OBICO_ONNX_BYTES
        } else {
            progress.total
        },
        message: if progress.message.is_empty() {
            "The failure model is not downloaded. The built-in detector is in use.".into()
        } else {
            progress.message.clone()
        },
    }
}

#[tauri::command]
pub fn camera_model_status(app: AppHandle) -> Result<ModelStatus, String> {
    let path = model_file(&app)?;
    Ok(status_of(&path))
}

/// Download the published weights into the app data directory. The catalogue
/// cache is never the destination. A second click while a download is running
/// reports the same progress instead of starting another copy.
#[tauri::command]
pub fn camera_model_download(app: AppHandle) -> Result<ModelStatus, String> {
    let path = model_file(&app)?;
    if camera::model_is_verified(&path) {
        return Ok(status_of(&path));
    }
    {
        let progress = model_progress().lock().map_err(|_| "the model lock is poisoned".to_string())?;
        if progress.phase == "downloading" {
            drop(progress);
            return Ok(status_of(&path));
        }
    }
    {
        let mut progress = model_progress().lock().map_err(|_| "the model lock is poisoned".to_string())?;
        progress.phase = "downloading".into();
        progress.received = 0;
        progress.total = camera::OBICO_ONNX_BYTES;
        progress.message = "Downloading the failure model.".into();
    }
    let dest = path.clone();
    thread::spawn(move || {
        let result = camera::install_model(
            camera::release_url(),
            &dest,
            camera::OBICO_ONNX_SHA256,
            Some(camera::OBICO_ONNX_BYTES),
            |received, total| {
                if let Ok(mut progress) = model_progress().lock() {
                    progress.received = received;
                    if let Some(total) = total {
                        progress.total = total;
                    }
                }
            },
        );
        if let Ok(mut progress) = model_progress().lock() {
            match result {
                Ok(()) => {
                    progress.phase = "ready".into();
                    progress.message = "The failure model is ready.".into();
                }
                Err(err) => {
                    progress.phase = "failed".into();
                    progress.message = err;
                }
            }
        }
    });
    Ok(status_of(&path))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecStatus {
    pub phase: String,
    pub ready: bool,
    pub from_source: bool,
    pub received: u64,
    pub total: u64,
    pub credit: String,
    pub message: String,
}

struct CodecProgress {
    phase: String,
    received: u64,
    total: u64,
    message: String,
}

fn codec_progress() -> &'static Mutex<CodecProgress> {
    static PROGRESS: Mutex<CodecProgress> = Mutex::new(CodecProgress {
        phase: String::new(),
        received: 0,
        total: 0,
        message: String::new(),
    });
    &PROGRESS
}

fn codec_status_of(app: &AppHandle) -> Result<CodecStatus, String> {
    let credit = camera::OPENH264_CREDIT.to_string();
    if camera::codec_from_source() {
        return Ok(CodecStatus {
            phase: "ready".into(),
            ready: true,
            from_source: true,
            received: 0,
            total: 0,
            credit,
            message: "This build compiles OpenH264 from source. Cisco's patent promise does not cover that build.".into(),
        });
    }
    let dir = codec_dir(app)?;
    let path = camera::codec_library_path(&dir)?;
    let asset = camera::codec_asset()?;
    if camera::codec_is_verified(&path, asset.sha256) {
        let (received, total) = codec_counts();
        return Ok(CodecStatus {
            phase: "ready".into(),
            ready: true,
            from_source: false,
            received,
            total,
            credit,
            message: "Cisco's OpenH264 library is ready.".into(),
        });
    }
    let progress = codec_progress()
        .lock()
        .map_err(|_| "the codec lock is poisoned".to_string())?;
    let phase = if progress.phase.is_empty() {
        "missing"
    } else {
        progress.phase.as_str()
    };
    let message = if progress.message.is_empty() {
        "Cisco's OpenH264 library is not installed yet. Shale downloads it on first use."
    } else {
        progress.message.as_str()
    };
    Ok(CodecStatus {
        phase: phase.into(),
        ready: false,
        from_source: false,
        received: progress.received,
        total: progress.total,
        credit,
        message: message.into(),
    })
}

fn codec_counts() -> (u64, u64) {
    codec_progress()
        .lock()
        .map(|progress| (progress.received, progress.total))
        .unwrap_or((0, 0))
}

#[tauri::command]
pub fn camera_codec_status(app: AppHandle) -> Result<CodecStatus, String> {
    codec_status_of(&app)
}

/// Download Cisco's binary into the app data directory. A test does not call
/// this with Cisco's URL. A second click while a download is running reports
/// the same progress instead of starting another copy.
#[tauri::command]
pub fn camera_codec_download(app: AppHandle) -> Result<CodecStatus, String> {
    if camera::codec_from_source() {
        return codec_status_of(&app);
    }
    let dir = codec_dir(&app)?;
    let dest = camera::codec_library_path(&dir)?;
    let asset = camera::codec_asset()?;
    if camera::codec_is_verified(&dest, asset.sha256) {
        return codec_status_of(&app);
    }
    {
        let progress = codec_progress()
            .lock()
            .map_err(|_| "the codec lock is poisoned".to_string())?;
        if progress.phase == "downloading" {
            drop(progress);
            return codec_status_of(&app);
        }
    }
    {
        let mut progress = codec_progress()
            .lock()
            .map_err(|_| "the codec lock is poisoned".to_string())?;
        progress.phase = "downloading".into();
        progress.received = 0;
        progress.total = 0;
        progress.message = "Downloading OpenH264 from Cisco.".into();
    }
    let url = camera::codec_download_url(&asset);
    let sha = asset.sha256.to_string();
    thread::spawn(move || {
        let result = camera::fetch_codec(&url, &dest, &sha, |received, total| {
            if let Ok(mut progress) = codec_progress().lock() {
                progress.received = received;
                if let Some(total) = total {
                    progress.total = total;
                }
            }
        });
        if let Ok(mut progress) = codec_progress().lock() {
            match result {
                Ok(()) => {
                    progress.phase = "ready".into();
                    progress.message = "Cisco's OpenH264 library is ready.".into();
                }
                Err(err) => {
                    progress.phase = "failed".into();
                    progress.message = err;
                }
            }
        }
    });
    codec_status_of(&app)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelapseView {
    pub frames: usize,
    pub directory: String,
    pub preview: String,
    pub video: Option<String>,
    pub note: String,
}

/// Write the frames the window has kept. The folder is the one the user chose.
/// The catalogue cache is refused. The MP4 is written only when Cisco's
/// library is already installed; this command does not download it.
#[tauri::command]
pub fn camera_timelapse_save(
    app: AppHandle,
    folder: String,
    name: String,
    frames: Vec<String>,
    interval_ms: u32,
) -> Result<TimelapseView, String> {
    if frames.is_empty() {
        return Err("There are no frames to save.".into());
    }
    if frames.len() > 180 {
        return Err("A timelapse keeps at most 180 frames.".into());
    }
    let mut jpegs = Vec::with_capacity(frames.len());
    for frame in &frames {
        let raw = frame
            .split_once(',')
            .map(|(_, body)| body)
            .unwrap_or(frame.as_str());
        jpegs.push(camera::base64_decode(raw)?);
    }
    let library = codec_library(&app);
    let saved = camera::save_timelapse(
        std::path::Path::new(&folder),
        &name,
        &jpegs,
        library.as_deref(),
        interval_ms,
    )?;
    Ok(TimelapseView {
        frames: saved.frames,
        directory: saved.directory.display().to_string(),
        preview: saved.preview.display().to_string(),
        video: saved.video.map(|path| path.display().to_string()),
        note: saved.note,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureWarning {
    pub id: String,
    pub note: String,
    pub frames: Vec<String>,
}

fn warning_views() -> Result<Vec<FailureWarning>, String> {
    Ok(camera::list_warnings(&camera::data_root())?
        .into_iter()
        .map(|item| FailureWarning {
            id: item.id,
            note: item.note,
            frames: item.frames.iter().map(|jpeg| camera::base64(jpeg)).collect(),
        })
        .collect())
}

/// Keep the frames from just before a warning. The folder is the app data
/// directory, or `SHALE_CACHE_DIR` when that is set. Older warnings are pruned.
#[tauri::command]
pub fn camera_failure_save(note: String, frames: Vec<String>) -> Result<Vec<FailureWarning>, String> {
    if frames.is_empty() {
        return Err("There is no JPEG to keep with this warning.".into());
    }
    let mut jpegs = Vec::with_capacity(frames.len());
    for frame in &frames {
        let raw = frame
            .split_once(',')
            .map(|(_, body)| body)
            .unwrap_or(frame.as_str());
        jpegs.push(camera::base64_decode(raw)?);
    }
    camera::save_warning(&camera::data_root(), &note, &jpegs)?;
    warning_views()
}

#[tauri::command]
pub fn camera_failure_history() -> Result<Vec<FailureWarning>, String> {
    warning_views()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SensitivityPreviewView {
    pub jpeg: String,
    pub kind: String,
    pub confidence: f64,
    pub note: String,
}

/// Judge a generated test frame at the chosen sensitivity. No printer is used.
#[tauri::command]
pub fn camera_sensitivity_preview(sensitivity: String) -> Result<SensitivityPreviewView, String> {
    let preview = camera::preview_sensitivity(camera::Sensitivity::parse(&sensitivity))?;
    Ok(SensitivityPreviewView {
        jpeg: camera::base64(&preview.jpeg),
        kind: preview.finding.kind.as_str().into(),
        confidence: preview.finding.confidence,
        note: preview.finding.note,
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
