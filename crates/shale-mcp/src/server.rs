use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::{
    handler::server::{
        router::{prompt::PromptRouter, tool::ToolRouter},
        wrapper::Parameters,
    },
    model::*,
    prompt, prompt_handler, prompt_router, schemars,
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use serde::Deserialize;
use serde_json::json;
use shale_core::{
    CustomInstruction, Session, SettingsPatch, SlotQuery, SuggestQuery, SupportParams,
    SupportPlan,
};

use crate::nonplanar::{self, NonplanarSettings};
use crate::printers::{AccountPrinters, PrinterDirectory};

#[derive(Clone)]
pub struct ShaleMcp {
    session: Arc<Mutex<Session>>,
    printers: Arc<dyn PrinterDirectory>,
    /// Non-planar options last set through `set_settings`. `shale_core` has no
    /// getter for them, and a tool must be able to report what the job will do.
    nonplanar: Arc<Mutex<NonplanarSettings>>,
    /// Progress for the slice running outside the session lock.
    slice_job: Arc<Mutex<SliceJob>>,
    // The handler macros read these. Derived Clone hides that from dead-code analysis.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    #[allow(dead_code)]
    prompt_router: PromptRouter<Self>,
}

impl ShaleMcp {
    pub fn new(session: Session) -> Self {
        Self::with_printers(session, Arc::new(AccountPrinters))
    }

    pub fn with_printers(session: Session, printers: Arc<dyn PrinterDirectory>) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
            printers,
            nonplanar: Arc::new(Mutex::new(NonplanarSettings::default())),
            slice_job: Arc::new(Mutex::new(SliceJob::default())),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Session>, McpError> {
        self.session
            .lock()
            .map_err(|_| McpError::internal_error("Shale session lock was poisoned", None))
    }

    fn nonplanar(&self) -> Result<NonplanarSettings, McpError> {
        self.nonplanar
            .lock()
            .map(|guard| *guard)
            .map_err(|_| McpError::internal_error("non-planar settings lock was poisoned", None))
    }
}

fn ok_json(value: impl serde::Serialize) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(&value)
        .map_err(|err| McpError::internal_error(err.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

fn fail(err: impl std::fmt::Display) -> McpError {
    McpError::internal_error(err.to_string(), None)
}

/// How long `slice` waits before telling the caller to poll.
///
/// A cube usually finishes inside this. The aerospike takes about ten minutes,
/// which is longer than a client's 600 second tool timeout, so the call returns
/// `running` and the caller polls.
const SLICE_INLINE_WAIT: Duration = Duration::from_secs(2);

struct SliceJob {
    generation: u64,
    status: &'static str,
    fraction: f64,
    message: String,
    summary: Option<serde_json::Value>,
    error: Option<String>,
    partial: Option<shale_core::ExportSnapshot>,
    warnings: Vec<String>,
}

impl Default for SliceJob {
    fn default() -> Self {
        Self {
            generation: 0,
            status: "idle",
            fraction: 0.0,
            message: String::new(),
            summary: None,
            error: None,
            partial: None,
            warnings: Vec::new(),
        }
    }
}

fn slice_status(job: &SliceJob) -> serde_json::Value {
    if job.status == "done" {
        if let Some(summary) = &job.summary {
            if summary.is_object() {
                let mut value = summary.clone();
                if let Some(object) = value.as_object_mut() {
                    object.insert("status".into(), json!("done"));
                    object.insert("fraction".into(), json!(job.fraction));
                    object.insert("message".into(), json!(job.message));
                }
                return value;
            }
            return json!({
                "status": "done",
                "fraction": 1.0,
                "message": job.message,
                "plates": summary,
            });
        }
    }
    let mut value = json!({
        "status": job.status,
        "fraction": job.fraction,
        "message": job.message,
        "error": job.error,
    });
    if !job.warnings.is_empty() {
        value["warnings"] = json!(job.warnings);
    }
    if let Some(partial) = &job.partial {
        value["partial"] = json!(true);
        value["layers_written"] = json!(partial.layers_written);
        value["estimated_layers"] = json!(partial.estimated_layers);
        value["print_time_seconds"] = json!(partial.print_time_seconds);
        value["filament_metres"] = json!(partial.filament_metres);
        value["note"] = json!(
            "Export is still writing the G-code. print_time_seconds and filament_metres are from the file so far, not the finished header."
        );
    }
    value
}

fn note_progress(job: &Mutex<SliceJob>, generation: u64, progress: &shale_core::EngineProgress) {
    if let Ok(mut job) = job.lock() {
        if job.generation == generation && job.status == "running" {
            job.fraction = f64::from(progress.fraction);
            job.message = progress.message.clone();
            if progress.partial.is_some() {
                job.partial = progress.partial.clone();
            }
        }
    }
}

fn spawn_slice(
    session: Arc<Mutex<shale_core::Session>>,
    job: Arc<Mutex<SliceJob>>,
    generation: u64,
    plate: Option<u32>,
    all_plates: bool,
    ignore: Option<bool>,
) {
    shale_core::clear_engine_cancel();
    let worker = Arc::clone(&job);
    let started = std::thread::Builder::new()
        .name("shale-slice".into())
        .spawn(move || {
            let result = run_slice(
                &session,
                &worker,
                generation,
                plate,
                all_plates,
                ignore,
            );
            if let Err(err) = result {
                if let Ok(mut job) = worker.lock() {
                    if job.generation == generation {
                        job.status = "failed";
                        job.error = Some(err);
                    }
                }
            }
        });
    if started.is_err() {
        if let Ok(mut job) = job.lock() {
            if job.generation == generation {
                job.status = "failed";
                job.error = Some("could not start the slice".into());
            }
        }
    }
}

/// Lock order is the session, then the slice job. Progress updates take only
/// the slice job, so a poll does not wait for the engine.
fn run_slice(
    session: &Arc<Mutex<shale_core::Session>>,
    job: &Arc<Mutex<SliceJob>>,
    generation: u64,
    plate: Option<u32>,
    all_plates: bool,
    ignore: Option<bool>,
) -> Result<(), String> {
    let plates = {
        let mut session = session
            .lock()
            .map_err(|_| "Shale session lock was poisoned".to_string())?;
        if let Some(ignore) = ignore {
            session.set_ignore_project_settings(ignore);
        }
        if all_plates {
            let listed = session.list_plates();
            if listed.len() > 1 {
                listed.into_iter().map(|plate| plate.number).collect()
            } else {
                Vec::new()
            }
        } else {
            if let Some(number) = plate {
                session
                    .reload_plate(number)
                    .map_err(|err| err.to_string())?;
            }
            Vec::new()
        }
    };
    if plates.is_empty() {
        let prepared = {
            let session = session
                .lock()
                .map_err(|_| "Shale session lock was poisoned".to_string())?;
            session.prepare_slice().map_err(|err| err.to_string())?
        };
        note_bed(session, job, generation)?;
        let outcome = prepared
            .run(&mut |progress| {
                note_progress(job, generation, &progress);
            })
            .map_err(|err| err.to_string())?;
        store_outcome(session, job, generation, outcome, true)?;
        return Ok(());
    }
    let count = plates.len();
    let mut summaries = Vec::with_capacity(count);
    for (index, number) in plates.into_iter().enumerate() {
        if !still_current(job, generation)? {
            return Ok(());
        }
        let prepared = {
            let mut session = session
                .lock()
                .map_err(|_| "Shale session lock was poisoned".to_string())?;
            session
                .reload_plate(number)
                .map_err(|err| err.to_string())?;
            session.prepare_slice().map_err(|err| err.to_string())?
        };
        note_bed(session, job, generation)?;
        let outcome = prepared
            .run(&mut |progress| {
                let mut progress = progress;
                progress.fraction = ((index as f32) + progress.fraction) / count as f32;
                note_progress(job, generation, &progress);
            })
            .map_err(|err| err.to_string())?;
        if let Some(summary) = store_outcome(session, job, generation, outcome, false)? {
            summaries.push(summary);
        } else {
            return Ok(());
        }
    }
    let mut job = job
        .lock()
        .map_err(|_| "slice lock was poisoned".to_string())?;
    if job.generation == generation {
        job.summary = Some(serde_json::Value::Array(summaries));
        job.status = "done";
        job.fraction = 1.0;
        job.message = "done".into();
    }
    Ok(())
}

fn note_bed(
    session: &Mutex<shale_core::Session>,
    job: &Mutex<SliceJob>,
    generation: u64,
) -> Result<(), String> {
    let warnings = session
        .lock()
        .map_err(|_| "Shale session lock was poisoned".to_string())?
        .bed_warnings();
    let mut job = job
        .lock()
        .map_err(|_| "slice lock was poisoned".to_string())?;
    if job.generation == generation && job.status == "running" {
        job.warnings = warnings;
    }
    Ok(())
}

fn still_current(job: &Mutex<SliceJob>, generation: u64) -> Result<bool, String> {
    let job = job
        .lock()
        .map_err(|_| "slice lock was poisoned".to_string())?;
    Ok(job.generation == generation)
}

fn store_outcome(
    session: &Arc<Mutex<shale_core::Session>>,
    job: &Arc<Mutex<SliceJob>>,
    generation: u64,
    outcome: shale_core::SliceOutcome,
    publish: bool,
) -> Result<Option<serde_json::Value>, String> {
    let mut session = session
        .lock()
        .map_err(|_| "Shale session lock was poisoned".to_string())?;
    let mut job = job
        .lock()
        .map_err(|_| "slice lock was poisoned".to_string())?;
    if job.generation != generation {
        return Ok(None);
    }
    let summary = session.commit_slice(outcome);
    let mut value = slice_summary_json(&summary).map_err(|err| err.to_string())?;
    if let Some(object) = value.as_object_mut() {
        let mut warnings = object
            .get("warnings")
            .and_then(|item| item.as_array())
            .cloned()
            .unwrap_or_default();
        for warning in session.bed_warnings() {
            if !warnings.iter().any(|existing| existing.as_str() == Some(warning.as_str())) {
                warnings.push(serde_json::Value::String(warning));
            }
        }
        object.insert("warning_count".into(), serde_json::json!(warnings.len()));
        object.insert("warnings".into(), serde_json::Value::Array(warnings));
    }
    if publish {
        job.summary = Some(value.clone());
        job.status = "done";
        job.fraction = 1.0;
        job.message = "done".into();
    }
    Ok(Some(value))
}

/// Slice summary plus the non-planar digest, `null` for a purely planar slice.
///
/// The raw `nonplanar_report` (and its preview float arrays, and the full
/// violation list) is dropped: `nonplanar` carries the same decisions in a
/// shape a model can read, and a fallback job otherwise answers with hundreds
/// of kilobytes of collision hits.
fn slice_summary_json(
    summary: &shale_core::JobSliceSummary,
) -> Result<serde_json::Value, McpError> {
    let mut value = serde_json::to_value(summary).map_err(fail)?;
    let digest = match &summary.nonplanar_report {
        Some(report) => nonplanar::report_digest(report),
        None => serde_json::Value::Null,
    };
    if let Some(object) = value.as_object_mut() {
        object.remove("nonplanar_report");
        object.insert("nonplanar".into(), digest);
    }
    Ok(value)
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PrinterArgs {
    /// Printer definition id, such as `bambu_lab_a1`. Omit to use the current printer.
    printer_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LoadModelArgs {
    /// Filesystem path to an STL or 3MF file.
    path: String,
    /// 1-based plate of a multi-plate 3MF. Omit to load plate 1.
    #[serde(default)]
    plate: Option<u32>,
    /// Leave the file's embedded process settings unused.
    #[serde(default)]
    ignore_project_settings: Option<bool>,
    /// Keep models already on the plate and append this file. Without it, a 3MF replaces the plate. An STL is added either way.
    #[serde(default)]
    add_to_plate: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SliceArgs {
    /// Slice this plate of the loaded 3MF instead of the one currently on the bed.
    #[serde(default)]
    plate: Option<u32>,
    /// Slice every plate. The result is a list of slice summaries.
    #[serde(default)]
    all_plates: Option<bool>,
    /// Leave the file's embedded process settings unused.
    #[serde(default)]
    ignore_project_settings: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TransformArgs {
    id: u32,
    translation: Option<[f64; 3]>,
    rotation_deg: Option<[f64; 3]>,
    scale: Option<[f64; 3]>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ModelIdArgs {
    id: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExplainArgs {
    /// Setting keys to explain. Empty returns the usual slice settings.
    #[serde(default)]
    keys: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ExportArgs {
    /// Destination path for the G-code file.
    path: String,
    /// Row index from compare_slices. Omit for the last slice.
    #[serde(default)]
    variant: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RemoveArgs {
    id: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SettingsArgs {
    printer_id: Option<String>,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    layer_height: Option<f64>,
    infill_sparse_density: Option<f64>,
    support_enable: Option<bool>,
    material_print_temperature: Option<f64>,
    material_bed_temperature: Option<f64>,
    /// `off`, `curved-tops`, or `auto`. Non-planar needs one of the toolhead
    /// envelopes (Bambu A1, A1 mini, P1P, P1S, X1C, H2D); other printers are
    /// refused with the list, and an unknown mode is an error rather than a
    /// silent planar slice.
    nonplanar_mode: Option<String>,
    /// Maximum surface slope in degrees: greater than 0 and at most 10, the
    /// clearance cap the shipped toolhead envelopes are calibrated for.
    nonplanar_max_angle_deg: Option<f64>,
    /// Error out instead of falling back to a planar slice when the curved
    /// pass cannot be verified.
    nonplanar_strict: Option<bool>,
    /// Bed adhesion: `none`, `skirt`, or `brim`. Example: { "adhesion_type": "brim", "brim_width": 8 }.
    adhesion_type: Option<String>,
    /// Brim width in millimetres, used when adhesion_type is brim.
    brim_width: Option<f64>,
    /// Support style: `normal` or `tree`. `auto` is named here and refused until the engine has it.
    support_structure: Option<String>,
    /// Any other Cura setting, the same as the CLI `--set KEY=VALUE`. An unknown key is an error that suggests a close key. Example: { "settings": { "draft_shield": "limited" } }.
    #[serde(default)]
    settings: std::collections::BTreeMap<String, String>,
}

impl From<SettingsArgs> for SettingsPatch {
    fn from(args: SettingsArgs) -> Self {
        Self {
            printer_id: args.printer_id,
            variant_name: args.variant_name,
            quality_type: args.quality_type,
            material_id: args.material_id,
            layer_height: args.layer_height,
            infill_sparse_density: args.infill_sparse_density,
            support_enable: args.support_enable,
            material_print_temperature: args.material_print_temperature,
            material_bed_temperature: args.material_bed_temperature,
            nonplanar_mode: args.nonplanar_mode,
            nonplanar_max_angle_deg: args.nonplanar_max_angle_deg,
            nonplanar_strict: args.nonplanar_strict,
            adhesion_type: args.adhesion_type,
            brim_width: args.brim_width,
            support_structure: args.support_structure,
            settings: args.settings,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct InstructionArgs {
    /// pause, temperature, fan, beep, message, or park_and_wait.
    kind: String,
    layer: Option<u32>,
    height_mm: Option<f64>,
    filament_change: Option<bool>,
    message: Option<String>,
    nozzle_c: Option<f64>,
    bed_c: Option<f64>,
    wait: Option<bool>,
    percent: Option<f64>,
    text: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
}

impl InstructionArgs {
    fn into_instruction(self) -> Result<CustomInstruction, McpError> {
        match self.kind.as_str() {
            "pause" => Ok(CustomInstruction::Pause {
                layer: self.layer,
                height_mm: self.height_mm,
                filament_change: self.filament_change.unwrap_or(false),
                message: self.message,
            }),
            "temperature" => Ok(CustomInstruction::Temperature {
                layer: self
                    .layer
                    .ok_or_else(|| fail("temperature needs a layer"))?,
                nozzle_c: self.nozzle_c,
                bed_c: self.bed_c,
                wait: self.wait.unwrap_or(false),
            }),
            "fan" => Ok(CustomInstruction::Fan {
                height_mm: self.height_mm.ok_or_else(|| fail("fan needs height_mm"))?,
                percent: self.percent.ok_or_else(|| fail("fan needs percent"))?,
            }),
            "beep" => Ok(CustomInstruction::Beep {
                layer: self.layer.ok_or_else(|| fail("beep needs a layer"))?,
            }),
            "message" => Ok(CustomInstruction::Message {
                layer: self.layer.ok_or_else(|| fail("message needs a layer"))?,
                text: self.text.ok_or_else(|| fail("message needs text"))?,
            }),
            "park_and_wait" => Ok(CustomInstruction::ParkAndWait {
                layer: self
                    .layer
                    .ok_or_else(|| fail("park_and_wait needs a layer"))?,
                x: self.x,
                y: self.y,
                message: self.message,
            }),
            other => Err(fail(format!("unknown instruction kind {other}"))),
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SupportQuery {
    id: u32,
    /// Cura's overhang angle, from the vertical. 50 is the usual default.
    overhang_angle_deg: Option<f64>,
    /// Spans up to this length, held at both ends, are bridges and are not supported.
    max_bridge_mm: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ApplySupportArgs {
    /// Region ids to keep. Omit to enable every proposed region.
    ids: Option<Vec<u32>>,
}

fn support_params(query: &SupportQuery) -> SupportParams {
    let mut params = SupportParams::default();
    if let Some(value) = query.overhang_angle_deg {
        params.overhang_angle_deg = value;
    }
    if let Some(value) = query.max_bridge_mm {
        params.max_bridge_mm = value;
    }
    params
}

fn plan_for_tool(plan: &SupportPlan) -> SupportPlan {
    let mut plan = plan.clone();
    for span in &mut plan.spans {
        span.samples.clear();
    }
    plan
}

fn slot_loaded() -> bool {
    true
}

fn suggest_slot(item: SuggestSlot) -> SlotQuery {
    SlotQuery {
        index: item.index,
        filament_type: item.filament_type,
        colour: item.colour,
        material_id: item.material_id,
        loaded: item.loaded,
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SuggestSlot {
    /// Zero-based spool index. 0 is the plate material.
    index: usize,
    /// PLA, PETG, TPU, ABS, ASA, PA, PVA, or a profile name containing one of those.
    filament_type: String,
    #[serde(default)]
    colour: String,
    #[serde(default)]
    material_id: String,
    /// False for an empty bay.
    #[serde(default = "slot_loaded")]
    loaded: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SuggestArgs {
    /// Spools the project is using. Omit to use the job's material as PLA-or-whatever its id says.
    #[serde(default)]
    slots: Vec<SuggestSlot>,
    /// Spools the connected printer has loaded. Omit when no printer is connected.
    #[serde(default)]
    device_slots: Option<Vec<SuggestSlot>>,
    /// AMS humidity from 0 to 100. Omit when the printer did not report it.
    humidity_percent: Option<f64>,
    /// Hours since a dry cycle, from spool data only. Omit when unknown.
    hours_since_dry: Option<f64>,
    /// Explicit tags. The only one that changes a suggestion is `hot-use`.
    #[serde(default)]
    tags: Vec<String>,
    /// Suggestion or rule ids dismissed for this project.
    #[serde(default)]
    dismissed: Vec<String>,
    /// Rule ids dismissed for every project.
    #[serde(default)]
    global_dismissed: Vec<String>,
    /// Rule ids already shown once. `filament_fit` is the one that uses this.
    #[serde(default)]
    shown_once: Vec<String>,
    /// One of `off`, `auto`, `curved-tops`.
    nonplanar_mode: Option<String>,
    /// True when the printer has an AMS or more than one extruder.
    multi_material: Option<bool>,
    /// Maximum suggestions to return. Omit for the full ranked list.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SuggestionActionArgs {
    /// Suggestion id or rule id from get_suggestions.
    id: String,
    /// dismiss_suggestion only. true remembers the rule for every project.
    #[serde(default)]
    forever: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PrinterIdArgs {
    /// Printer id from list_my_printers.
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProjectPathArgs {
    /// Absolute path to a .shale project.
    path: String,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct StatsArgs {
    /// Filament price per kilogram. Omit for 20 EUR.
    price_per_kg: Option<f64>,
    /// Include one duration per layer.
    #[serde(default)]
    layers: Option<bool>,
    /// Row index from compare_slices. Omit for the last slice.
    #[serde(default)]
    variant: Option<usize>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct VariantPick {
    /// Row index from compare_slices. Omit for the last slice.
    #[serde(default)]
    variant: Option<usize>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct DescribeArgs {
    /// Tool names to describe. Up to 32. Omit to list every name.
    #[serde(default)]
    names: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct VariantArgs {
    /// Each patch is sliced on its own. At most 8.
    patches: Vec<SettingsArgs>,
    /// How many slices run at once. Omit for 2, or 3 when the machine has the memory. Never more than 3.
    concurrency: Option<usize>,
    /// Filament price per kilogram, used in the comparison. Omit for 20 EUR.
    price_per_kg: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct VariantIndexArgs {
    /// Row index from slice_variants, starting at 0.
    index: usize,
}

impl SuggestArgs {
    fn into_query(self) -> SuggestQuery {
        SuggestQuery {
            slots: self.slots.into_iter().map(suggest_slot).collect(),
            device_slots: self
                .device_slots
                .map(|slots| slots.into_iter().map(suggest_slot).collect()),
            humidity_percent: self.humidity_percent,
            hours_since_dry: self.hours_since_dry,
            tags: self.tags,
            dismissed: self.dismissed,
            global_dismissed: self.global_dismissed,
            shown_once: self.shown_once,
            nonplanar_mode: self.nonplanar_mode,
            multi_material: self.multi_material,
            limit: self.limit,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SlicePromptArgs {
    /// What the part is, in a few words.
    part: String,
}

#[tool_router]
impl ShaleMcp {
    fn reset_job(&self) -> Result<CallToolResult, McpError> {
        shale_core::cancel_engine_children();
        let mut session = self.lock()?;
        session.clear_job();
        let models = session.models();
        let printer_id = session.get_settings(&[]).ok().map(|view| view.printer_id);
        {
            let mut job = self.slice_job.lock().map_err(|_| {
                McpError::internal_error("slice lock was poisoned", None)
            })?;
            job.generation = job.generation.saturating_add(1);
            job.status = "idle";
            job.fraction = 0.0;
            job.message.clear();
            job.summary = None;
            job.error = None;
            job.partial = None;
            job.warnings.clear();
        }
        drop(session);
        let mut nonplanar = self.nonplanar.lock().map_err(|_| {
            McpError::internal_error("non-planar settings lock was poisoned", None)
        })?;
        *nonplanar = NonplanarSettings::default();
        shale_core::clear_engine_cancel();
        ok_json(json!({
            "cleared": true,
            "models": models,
            "printer_id": printer_id,
        }))
    }

    #[tool(description = "List printers available from the fetched Cura definitions.")]
    fn list_printers(&self) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(session.list_printers())
    }

    #[tool(description = "List materials whose diameter matches the current or given printer.")]
    fn list_materials(
        &self,
        Parameters(PrinterArgs { printer_id }): Parameters<PrinterArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(
            session
                .list_materials(printer_id.as_deref())
                .map_err(fail)?,
        )
    }

    #[tool(description = "List nozzle variants and quality profiles for a printer.")]
    fn list_profiles(
        &self,
        Parameters(PrinterArgs { printer_id }): Parameters<PrinterArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let (variants, qualities) = session.list_profiles(printer_id.as_deref()).map_err(fail)?;
        ok_json(json!({ "variants": variants, "qualities": qualities }))
    }

    #[tool(
        description = "Start a fresh job. Clears models, printer, settings, and non-planar mode. This process keeps one job until new_job or clear_job."
    )]
    fn new_job(&self) -> Result<CallToolResult, McpError> {
        self.reset_job()
    }

    #[tool(
        description = "Clear the current job. Same as new_job: printer, settings, models, and non-planar mode do not carry into the next task."
    )]
    fn clear_job(&self) -> Result<CallToolResult, McpError> {
        self.reset_job()
    }

    #[tool(description = "Remove one loaded model by id. Other models stay.")]
    fn remove_model(
        &self,
        Parameters(ModelIdArgs { id }): Parameters<ModelIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        let models = session.remove_model(id).map_err(fail)?;
        ok_json(json!({ "removed": id, "models": models }))
    }

    #[tool(
        description = "Load an STL or 3MF. An STL is added to the plate. A 3MF replaces the plate unless add_to_plate is true, which keeps models already loaded and appends this file. Call new_job for a fresh plate. A multi-plate file loads plate 1 unless plate is set; call list_plates first."
    )]
    fn load_model(
        &self,
        Parameters(LoadModelArgs {
            path,
            plate,
            ignore_project_settings,
            add_to_plate,
        }): Parameters<LoadModelArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        if let Some(ignore) = ignore_project_settings {
            session.set_ignore_project_settings(ignore);
        }
        ok_json(
            session
                .load_model_with(
                    std::path::Path::new(&path),
                    plate,
                    add_to_plate.unwrap_or(false),
                )
                .map_err(fail)?,
        )
    }

    #[tool(
        description = "List plates in the loaded file. Call this before slice with plate. number is the 1-based plate. An STL is plate 1."
    )]
    fn list_plates(&self) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(json!({ "plates": session.list_plates() }))
    }

    #[tool(
        description = "Set translation (mm), XYZ rotation (degrees), or scale for a loaded model. Omitted fields stay as they are."
    )]
    fn transform_model(
        &self,
        Parameters(TransformArgs {
            id,
            translation,
            rotation_deg,
            scale,
        }): Parameters<TransformArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(
            session
                .transform_model(id, translation, rotation_deg, scale)
                .map_err(fail)?,
        )
    }

    #[tool(
        description = "Keeps rotation and scale. Centres the model on the bed and drops it to Z = 0. Only the position changes."
    )]
    fn arrange_model(
        &self,
        Parameters(ModelIdArgs { id }): Parameters<ModelIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(session.arrange_model(id).map_err(fail)?)
    }

    #[tool(
        description = "Settings for the current job, including process settings embedded in a 3MF. User values win over the file. The nonplanar block is the job's mode, angle, and strict flag."
    )]
    fn get_settings(
        &self,
        Parameters(ExplainArgs { keys }): Parameters<ExplainArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let nonplanar = self.nonplanar()?;
        let view = session.get_settings(&keys).map_err(fail)?;
        let mut value = serde_json::to_value(&view).map_err(fail)?;
        value["nonplanar"] = nonplanar.to_json(&view.printer_id);
        ok_json(value)
    }

    #[tool(
        description = "Change printer, profile, or settings on the current job. They persist until new_job. adhesion_type is none, skirt, or brim. support_structure is normal or tree (auto is refused). settings is any other Cura key; a typo names the closest key. Non-planar mode is off, auto, or curved-tops, angle 0 < degrees <= 10."
    )]
    fn set_settings(
        &self,
        Parameters(patch): Parameters<SettingsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        let mut nonplanar = self.nonplanar()?;
        if let Some(mode) = patch.nonplanar_mode.as_deref() {
            nonplanar.mode = nonplanar::parse_mode(mode).map_err(fail)?;
        }
        if let Some(angle) = patch.nonplanar_max_angle_deg {
            nonplanar::check_max_angle(angle).map_err(fail)?;
            nonplanar.max_angle_deg = angle;
        }
        if let Some(strict) = patch.nonplanar_strict {
            nonplanar.strict = strict;
        }
        // The envelope check needs the printer the job will end up on, and the
        // patch may be changing it in this same call.
        let printer_id = match patch.printer_id.as_deref() {
            Some(id) => Some(id.to_string()),
            None => session
                .get_settings(&[])
                .ok()
                .map(|view| view.printer_id.clone()),
        };
        if let Some(printer_id) = printer_id {
            nonplanar::check_printer(nonplanar, &printer_id).map_err(fail)?;
        }
        session.set_settings(patch.into()).map_err(fail)?;
        {
            let mut stored = self.nonplanar.lock().map_err(|_| {
                McpError::internal_error("non-planar settings lock was poisoned", None)
            })?;
            *stored = nonplanar;
        }
        let view = session.get_settings(&[]).map_err(fail)?;
        let mut value = serde_json::to_value(&view).map_err(fail)?;
        value["nonplanar"] = nonplanar.to_json(&view.printer_id);
        ok_json(value)
    }

    #[tool(
        description = "Analyse one loaded model for overhangs, bridges, cantilevers, and islands. This does not change the job. Bridge and cantilever spans are the same map a later non-planar placement can use."
    )]
    fn analyse_overhangs(
        &self,
        Parameters(query): Parameters<SupportQuery>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let analysis = session
            .analyse_overhangs(query.id, support_params(&query))
            .map_err(fail)?;
        let spans: Vec<_> = analysis
            .spans
            .iter()
            .map(|span| {
                json!({
                    "kind": span.kind,
                    "length_mm": span.length_mm,
                    "width_mm": span.width_mm,
                    "area_mm2": span.area_mm2,
                    "z_mm": span.z_mm,
                    "angle_from_vertical_deg": span.angle_from_vertical_deg,
                    "anchor_ends": span.anchor_ends,
                    "samples": span.samples.len(),
                })
            })
            .collect();
        ok_json(json!({
            "bridges": analysis.count(shale_core::FeatureKind::Bridge),
            "cantilevers": analysis.count(shale_core::FeatureKind::Cantilever),
            "islands": analysis.count(shale_core::FeatureKind::Island),
            "overhangs": analysis.count(shale_core::FeatureKind::Overhang),
            "cosmetic": analysis.count(shale_core::FeatureKind::Cosmetic),
            "spans": spans,
        }))
    }

    #[tool(
        description = "Plan automatic non-planar curved-top bands for one model (no slice). Returns detected features, the chosen surfaces split into enabled and disabled with the reason for each, and a count of preview overlay triangles. Use nonplanar_mode=auto before slicing to apply the plan."
    )]
    fn plan_nonplanar(
        &self,
        Parameters(query): Parameters<SupportQuery>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let plan = session.plan_nonplanar_auto(query.id).map_err(fail)?;
        ok_json(nonplanar::plan_digest(&plan))
    }

    #[tool(
        description = "Propose support only where the analysis says it is needed. This does not turn supports on. Call apply_supports next (omit ids to enable every region). Bridges and cosmetic slopes become blockers. Cantilevers, islands, and other overhangs become support-mesh columns."
    )]
    fn propose_supports(
        &self,
        Parameters(query): Parameters<SupportQuery>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        let plan = session
            .propose_supports(query.id, support_params(&query))
            .map_err(fail)?;
        let mut value = serde_json::to_value(plan_for_tool(&plan)).map_err(fail)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "apply_supports".into(),
                json!("Call apply_supports to turn these regions on. Omit ids to enable them all, or pass the region ids to keep. This plan is not applied yet."),
            );
        }
        ok_json(value)
    }

    #[tool(
        description = "Enable a subset of the proposed support regions. Omit ids to enable them all. The next slice sends the enabled columns and blockers to CuraEngine as support_mesh and anti_overhang_mesh objects."
    )]
    fn apply_supports(
        &self,
        Parameters(ApplySupportArgs { ids }): Parameters<ApplySupportArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        let plan = session.apply_supports(ids.as_deref()).map_err(fail)?;
        ok_json(plan_for_tool(&plan))
    }

    #[tool(
        description = "List custom instructions stored on this job. They are compiled into G-code after CuraEngine runs."
    )]
    fn list_instructions(&self) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(session.instructions())
    }

    #[tool(
        description = "Add a custom instruction. Kinds: pause (layer or height_mm, filament_change, message), temperature (layer, nozzle_c, bed_c, wait), fan (height_mm, percent), beep (layer), message (layer, text), park_and_wait (layer, x, y, message)."
    )]
    fn add_instruction(
        &self,
        Parameters(args): Parameters<InstructionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let instruction = args.into_instruction()?;
        let mut session = self.lock()?;
        ok_json(session.add_instruction(instruction))
    }

    #[tool(description = "Remove a custom instruction by the id returned from add_instruction.")]
    fn remove_instruction(
        &self,
        Parameters(RemoveArgs { id }): Parameters<RemoveArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        session.remove_instruction(id).map_err(fail)?;
        ok_json(session.instructions())
    }

    #[tool(
        description = "Start a slice and return within a couple of seconds. If status is running, poll slice_progress, then slice_result. A cube is under a minute; the aerospike is about 10 minutes. Call list_plates before plate. To compare layer height or infill, use compare_slices instead of slicing each one."
    )]
    async fn slice(
        &self,
        Parameters(SliceArgs {
            plate,
            all_plates,
            ignore_project_settings,
        }): Parameters<SliceArgs>,
    ) -> Result<CallToolResult, McpError> {
        let generation = {
            let mut job = self.slice_job.lock().map_err(|_| {
                McpError::internal_error("slice lock was poisoned", None)
            })?;
            if job.status == "running" {
                return ok_json(slice_status(&job));
            }
            job.generation = job.generation.saturating_add(1);
            job.status = "running";
            job.fraction = 0.0;
            job.message = "Starting".into();
            job.summary = None;
            job.error = None;
            job.partial = None;
            job.warnings.clear();
            job.generation
        };
        spawn_slice(
            Arc::clone(&self.session),
            Arc::clone(&self.slice_job),
            generation,
            plate,
            all_plates == Some(true),
            ignore_project_settings,
        );
        let deadline = tokio::time::Instant::now() + SLICE_INLINE_WAIT;
        loop {
            {
                let job = self.slice_job.lock().map_err(|_| {
                    McpError::internal_error("slice lock was poisoned", None)
                })?;
                if job.generation != generation || job.status != "running" {
                    return ok_json(slice_status(&job));
                }
                if tokio::time::Instant::now() >= deadline {
                    return ok_json(slice_status(&job));
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tool(
        description = "Progress of the slice started by slice. status is running, done, or failed. fraction is 0 to 1. During export the message is Exporting layer N of M, and print_time_seconds, filament_metres, and layers_written are from the G-code written so far."
    )]
    fn slice_progress(&self) -> Result<CallToolResult, McpError> {
        let job = self.slice_job.lock().map_err(|_| {
            McpError::internal_error("slice lock was poisoned", None)
        })?;
        ok_json(slice_status(&job))
    }

    #[tool(
        description = "Time, filament, layers, and warnings from the slice started by slice. While export is still writing, returns those figures so far with partial true. Errors when nothing has been sliced, or the slice failed."
    )]
    fn slice_result(&self) -> Result<CallToolResult, McpError> {
        let job = self.slice_job.lock().map_err(|_| {
            McpError::internal_error("slice lock was poisoned", None)
        })?;
        match job.status {
            "done" => ok_json(slice_status(&job)),
            "failed" => Err(fail(
                job.error
                    .clone()
                    .unwrap_or_else(|| "slice failed".to_string()),
            )),
            "running" => {
                if job.partial.is_some() || !job.warnings.is_empty() {
                    ok_json(slice_status(&job))
                } else {
                    Err(fail(
                        "the slice is still running; poll slice_progress, then slice_result",
                    ))
                }
            }
            _ => Err(fail("nothing has been sliced yet")),
        }
    }

    #[tool(
        description = "Return the summary of the latest slice without slicing again. Pass variant as the index from compare_slices to read that row instead."
    )]
    fn slice_summary(
        &self,
        Parameters(args): Parameters<VariantPick>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let summary = if let Some(index) = args.variant {
            session.variant_summary(index).map_err(fail)?
        } else {
            session
                .summary()
                .ok_or_else(|| fail("nothing has been sliced yet. After compare_slices, pass variant as the row index."))?
        };
        ok_json(slice_summary_json(&summary)?)
    }

    #[tool(
        description = "Suggestions for the loaded model. Use after load_model, arrange_model or set_settings, and again after slice: time and purge checks need a slice. Returns id, severity (tip or warning), title, reason, objects, and an optional fix. Example: {\"slots\":[{\"index\":0,\"filament_type\":\"PETG\"},{\"index\":1,\"filament_type\":\"PLA\"}],\"multi_material\":true}. No loaded model returns []."
    )]
    fn get_suggestions(
        &self,
        Parameters(args): Parameters<SuggestArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let found = session.suggestions(args.into_query()).map_err(fail)?;
        ok_json(found)
    }

    #[tool(
        description = "Apply one suggestion's fix to the open project, then hide that suggestion. Pass the suggestion id from get_suggestions. Example: {\"id\": \"brim_contact:model-1\"}."
    )]
    fn apply_suggestion(
        &self,
        Parameters(args): Parameters<SuggestionActionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        let applied = session
            .apply_suggestion(&args.id, SuggestQuery::default())
            .map_err(fail)?;
        ok_json(applied)
    }

    #[tool(
        description = "Hide a suggestion. forever true stores the rule for every project. Example: {\"id\": \"brim\", \"forever\": false}."
    )]
    fn dismiss_suggestion(
        &self,
        Parameters(args): Parameters<SuggestionActionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        let dismissed = session
            .dismiss_suggestion(&args.id, args.forever.unwrap_or(false))
            .map_err(fail)?;
        ok_json(dismissed)
    }

    #[tool(
        description = "List printers on the signed-in Bambu account. Read only. If this process is not signed in, the error is final: no printer access without Shale sign-in; do not attempt other routes."
    )]
    fn list_my_printers(&self) -> Result<CallToolResult, McpError> {
        ok_json(self.printers.printers().map_err(fail)?)
    }

    #[tool(
        description = "Read one printer: state, current job, progress, time left, temperatures, and AMS slots when the report has them. Example: {\"id\": \"01P00C000000000\"}. Read only. If this process is not signed in, that error is final: do not read a config file or open LAN MQTT."
    )]
    fn printer_status(
        &self,
        Parameters(args): Parameters<PrinterIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        ok_json(self.printers.status(&args.id).map_err(fail)?)
    }

    #[tool(
        description = "Recent and current print jobs from the signed-in account. Read only. Does not start, pause, or stop a print. If this process is not signed in, that error is final: no printer access without Shale sign-in; do not attempt other routes."
    )]
    fn list_print_jobs(&self) -> Result<CallToolResult, McpError> {
        ok_json(self.printers.jobs().map_err(fail)?)
    }

    #[tool(description = "Projects opened or saved recently, newest first. Uses SHALE_CONFIG_DIR, or SHALE_CACHE_DIR/session when that is set, otherwise the desktop app's config directory. Each entry has path, name, printer, and whether the file is missing.")]
    fn list_recent_projects(&self) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(session.list_recent_projects().map_err(fail)?)
    }

    #[tool(
        description = "Open a .shale project into this session, replacing the models that are loaded now. Example: {\"path\": \"/home/me/bracket.shale\"}."
    )]
    fn open_project(
        &self,
        Parameters(args): Parameters<ProjectPathArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(session.open_project(std::path::Path::new(&args.path)).map_err(fail)?)
    }

    #[tool(
        description = "Save the open models as a .shale project. Meshes are stored as STL. Example: {\"path\": \"/home/me/bracket.shale\"}."
    )]
    fn save_project(
        &self,
        Parameters(args): Parameters<ProjectPathArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(session.save_project(std::path::Path::new(&args.path)).map_err(fail)?)
    }

    #[tool(
        description = "Breakdown of a slice: walls, infill, supports, skirt, travel, other, grams per extruder, purge, layers, speeds, and cost. print_time_seconds is a feed-rate estimate. header_time_seconds is the CuraEngine ;TIME: header. layers true adds per-layer seconds. variant is the index from compare_slices."
    )]
    fn slice_stats(
        &self,
        Parameters(args): Parameters<StatsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        ok_json(
            session
                .slice_stats_variant(
                    args.price_per_kg,
                    args.layers.unwrap_or(false),
                    args.variant,
                )
                .map_err(fail)?,
        )
    }

    #[tool(
        description = "Compare settings in one call, such as layer height 0.12 vs 0.20 or infill 15 vs 30. Do not slice each one yourself. Up to 8 patches. Does not change the current settings. Each row says to call apply_variant with its index. slice_stats, slice_summary, and export_gcode take that index. Same as slice_variants."
    )]
    fn compare_slices(
        &self,
        Parameters(args): Parameters<VariantArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.compare_patches(args)
    }

    #[tool(
        description = "Compare settings in one call, such as layer height 0.12 vs 0.20 or infill 15 vs 30. Do not slice each one yourself. Up to 8 patches. Does not change the current settings. Same as compare_slices."
    )]
    fn slice_variants(
        &self,
        Parameters(args): Parameters<VariantArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.compare_patches(args)
    }

    fn compare_patches(&self, args: VariantArgs) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(
            session
                .slice_variants(
                    args.patches.into_iter().map(SettingsPatch::from).collect(),
                    args.concurrency,
                    args.price_per_kg,
                )
                .map_err(fail)?,
        )
    }

    #[tool(
        description = "Make one compared patch the current settings. index is the row from slice_variants. Example: {\"index\": 0}."
    )]
    fn apply_variant(
        &self,
        Parameters(args): Parameters<VariantIndexArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut session = self.lock()?;
        ok_json(session.apply_variant(args.index).map_err(fail)?)
    }

    #[tool(
        description = "Copy G-code to a filesystem path. Omit variant for the last slice. After compare_slices, pass the row index as variant."
    )]
    fn export_gcode(
        &self,
        Parameters(ExportArgs { path, variant }): Parameters<ExportArgs>,
    ) -> Result<CallToolResult, McpError> {
        let session = self.lock()?;
        let written = session
            .export_variant(std::path::Path::new(&path), variant)
            .map_err(fail)?;
        ok_json(json!({ "path": written }))
    }

    #[tool(
        description = "Describe tools by name. Accepts up to 32 names, more than 10. Omit names to list every tool name. Example: {\"names\": [\"apply_supports\", \"compare_slices\", \"slice_stats\"]}."
    )]
    fn tool_describe(
        &self,
        Parameters(args): Parameters<DescribeArgs>,
    ) -> Result<CallToolResult, McpError> {
        const LIMIT: usize = 32;
        let tools = self.tool_router.list_all();
        if args.names.len() > LIMIT {
            return Err(fail(format!(
                "tool_describe accepts at most {LIMIT} names"
            )));
        }
        if args.names.is_empty() {
            let names: Vec<_> = tools.iter().map(|tool| tool.name.to_string()).collect();
            return ok_json(json!({ "names": names, "limit": LIMIT }));
        }
        let mut described = Vec::new();
        let mut missing = Vec::new();
        for name in &args.names {
            match tools.iter().find(|tool| tool.name.as_ref() == name.as_str()) {
                Some(tool) => described.push(json!({
                    "name": tool.name,
                    "description": tool.description,
                })),
                None => missing.push(name.clone()),
            }
        }
        ok_json(json!({ "tools": described, "missing": missing, "limit": LIMIT }))
    }
}

#[prompt_router]
impl ShaleMcp {
    /// Steps an agent should follow to slice a part.
    #[prompt(name = "slice_a_model")]
    fn slice_a_model(
        &self,
        Parameters(SlicePromptArgs { part }): Parameters<SlicePromptArgs>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        let text = format!(
            "Slice \"{part}\" with Shale. Call new_job first so a previous printer, setting, or model is not reused. Use list_printers and set_settings, then load_model. Call list_plates before slice with a plate number. arrange_model keeps rotation and scale. If slice returns status running, poll slice_progress and then slice_result; a large model takes about 10 minutes. To compare layer heights or infill, call compare_slices once instead of slicing each setting. export_gcode writes the file. Do not edit the G-code by hand."
        );
        Ok(GetPromptResult::new(vec![PromptMessage::new_text(
            Role::User,
            text,
        )]))
    }
}

#[tool_handler]
#[prompt_handler]
impl ServerHandler for ShaleMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_instructions(
            "One Shale process keeps one job. Printer, settings, models, and non-planar mode stay until new_job or clear_job. A 3MF replaces the plate unless load_model sets add_to_plate. slice returns within a couple of seconds: if status is running, poll slice_progress. Export of a large model keeps moving and slice_progress carries partial time and filament before the file is finished. To compare layer height or infill, call compare_slices once, then apply_variant with the row index. slice_stats, slice_summary, and export_gcode take that same index. print_time_seconds is a feed-rate estimate; header_time_seconds is the CuraEngine header. If a printer tool says to sign in to Bambu in Shale first, that is the full answer: do not try another route. tool_describe accepts up to 32 names. Call list_plates before slice with a plate number. arrange_model keeps rotation and scale. propose_supports does not apply supports; call apply_supports.".to_string(),
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![Resource::new("shale://job", "Current Shale job")],
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if request.uri.as_str() != "shale://job" {
            return Err(McpError::resource_not_found(
                "unknown resource",
                Some(json!({ "uri": request.uri })),
            ));
        }
        let session = self.lock()?;
        let body = json!({
            "models": session.models(),
            "instructions": session.instructions(),
            "support": session.support_plan().map(plan_for_tool),
            "summary": session.summary(),
        });
        Ok(ReadResourceResult::new(vec![ResourceContents::text(
            body.to_string(),
            request.uri.clone(),
        )])
        .into())
    }
}
