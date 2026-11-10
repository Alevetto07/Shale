//! Tauri shell. All slicing behaviour lives in `shale-core`; this file only
//! holds window state and translates commands into that API.

// A Tauri command is a function of the arguments the window passes and the
// state it reads; the long ones (a slice, a queue job, a host connection) are
// honestly that wide, and a struct to hold them would move the same list one
// level away from the command that documents it.
#![allow(clippy::too_many_arguments)]
// The preview and settings payloads are tuples of primitives between here and
// `shale-core`; naming each one would add a type per call site.
#![allow(clippy::type_complexity)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use shale_core::{
    compare_support, drop_to_bed, forget_recent, load_mesh_with, load_named_bytes, load_recents,
    now_ms, overlays, place_on_bed, propose, read_project, remember_recent, set_enabled, slice,
    Catalog, CustomInstruction, Engine, EngineBackend, ImportReport, MaterialInfo, PlacedModel,
    PrinterInfo, ProjectData, ProjectModel, QualityInfo, RecentEntry, ResourcePaths,
    StoredInstruction, SupportOverlay, SupportParams, SupportPlan, TessellationTolerance, Toolpath,
    Transform, TriangleMesh, UserSettings, VariantInfo,
};
mod instance;
mod secrets;

use tauri::{AppHandle, Emitter, Manager, State};

struct Inner {
    catalog: Option<Catalog>,
    catalog_error: Option<String>,
    /// The profile library is read on a worker thread while the window comes
    /// up, and the reader stores it here when it is done. Commands never wait
    /// for it: on a cold disk the read has taken over a minute, and a command
    /// that parked under this lock turned that into a frozen window.
    catalog_loading: bool,
    models: Vec<LoadedModel>,
    /// The plates behind the one on screen. `models` is always the plate in
    /// front, and a stashed plate keeps its models and its support plan so a
    /// switch is a swap rather than a reload. `plates[active_plate]` is empty.
    plates: Vec<PlateStash>,
    active_plate: usize,
    /// Painted support, by model id: the triangles that ask for a column under
    /// them, and the triangles that ask for none. Kept beside the models rather
    /// than inside them, so a part keeps what was painted on it when it is
    /// copied, saved or read back.
    painted: std::collections::BTreeMap<u32, Paint>,
    /// The paint as it was before each stroke, newest last. A stroke is cheap —
    /// triangle indices, not geometry — so keeping a stack of them costs little
    /// and makes paint the one thing a brush must always be able to take back.
    paint_past: Vec<std::collections::BTreeMap<u32, Paint>>,
    /// Strokes that were taken back, newest last, so one can come forward again.
    paint_future: Vec<std::collections::BTreeMap<u32, Paint>>,
    next_id: u32,
    instructions: Vec<(u32, CustomInstruction)>,
    next_instruction: u32,
    support: Option<SupportPlan>,
    support_model_id: Option<u32>,
    engine: Engine,
    toolpath: Option<Toolpath>,
    gcode_path: Option<PathBuf>,
    /// Jobs waiting for the machine, oldest first. Each one is a copy of the
    /// sliced file, because slicing the next plate overwrites the first.
    queue: Vec<QueuedJob>,
    queue_next_id: u64,
    /// The queued job the machine is on, so the pane can say which of how many.
    queue_running: Option<String>,
    /// What the run is allowed to do. The rules live in `shale_core::queue`
    /// and are enforced here rather than in the pane, so a screen that forgot
    /// to ask cannot send a second job onto a bed that still holds the first.
    queue_run: shale_core::queue::QueueRun,
    /// The printer this window is pointed at, when one has been connected.
    host_config: Option<shale_host::HostConfig>,
    /// The printer the plate on screen was last resolved against. A send needs
    /// to know which machine it is talking to before it decides whether the
    /// G-code may go out at all.
    printer_id: Option<String>,
    slicing: bool,
    cancel_import: Arc<AtomicBool>,
    engine_message: String,
}

#[derive(Default)]
struct PlateStash {
    name: String,
    models: Vec<LoadedModel>,
    support: Option<SupportPlan>,
    support_model_id: Option<u32>,
}

struct LoadedModel {
    id: u32,
    name: String,
    mesh: TriangleMesh,
    transform: Transform,
    report: ImportReport,
    source_bytes: Vec<u8>,
    /// Deleted from the plate, but kept so that undo can bring it back without
    /// re-reading the file. Hidden models are never sliced, saved or exported.
    visible: bool,
    /// Per-part overrides, sent to the engine beside that part's mesh. Empty
    /// means the part follows the global settings.
    settings: std::collections::BTreeMap<String, String>,
    /// Which AMS spool prints this part. `None` is the machine's own extruder.
    slot: Option<usize>,
}

struct AppState(Mutex<Inner>);

#[derive(Serialize)]
struct Status {
    resources_ok: bool,
    resources_message: String,
    engine_path: String,
    engine_lib_dir: String,
    engine_ok: bool,
    engine_message: String,
    engine_backend: String,
    printer_count: usize,
    demo_model: Option<String>,
    demo: Option<String>,
    /// True while the profile library is still being read. The window is up
    /// and usable; the printer list arrives when this goes false.
    catalog_loading: bool,
    /// The version this binary was built as, so the window never has to
    /// hold a copy that can drift from Cargo.toml.
    version: String,
}

#[derive(Serialize)]
struct ModelView {
    id: u32,
    name: String,
    positions: Vec<f32>,
    transform: Transform,
    triangles: usize,
    max_deviation_mm: Option<f64>,
    max_turning_deg: Option<f64>,
    import_ms: u64,
    source: String,
    settings: std::collections::BTreeMap<String, String>,
    slot: Option<usize>,
}

#[derive(Serialize)]
struct Bed {
    width: f64,
    depth: f64,
    height: f64,
    center_is_zero: bool,
    layer_height: f64,
    infill_sparse_density: f64,
    support_enable: bool,
    nozzle_temperature: f64,
    bed_temperature: f64,
    nozzle_size: f64,
    line_width: f64,
    adhesion_type: String,
    speed_print: f64,
    /// The filament diameter the machine is built for. A material list that
    /// ignores it offers 2.85 mm spools for a 1.75 mm hotend, and picking one
    /// slices as whatever the resolver can find at the right diameter instead.
    material_diameter: f64,
    warning_count: usize,
}

#[derive(Serialize)]
struct SliceSummary {
    layer_count: usize,
    layers: Vec<LayerMark>,
    print_time_seconds: Option<f64>,
    /// Filament the job draws, in metres. `None` when a slice came back from
    /// disk with no measured figure in its header.
    filament_metres: Option<f64>,
    warning_count: usize,
    gcode_path: String,
    /// The parts the slice was made from. A job card may only describe the
    /// plate it was taken from.
    model_ids: Vec<u64>,
    /// The slice was started by the machine's own sequence rather than Cura's
    /// generic Marlin one. A Bambu machine may only be sent the former.
    machine_gcode: bool,
}

#[derive(Serialize)]
struct LayerMark {
    index: i32,
    z: f32,
}

#[derive(Serialize)]
struct PreviewPath {
    kind: String,
    points: Vec<[f32; 3]>,
    /// mm/s, so the preview can colour a line by how fast it is laid down.
    feed: f32,
}

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Inner>, String> {
    state
        .0
        .lock()
        .map_err(|_| "Shale state was poisoned".to_string())
}

/// Lend out the catalogue, or say honestly why it is not there yet. The read
/// runs on a thread of its own because it takes a second or two from a cold
/// disk, and a command that arrives before it finishes must not claim the
/// profiles are missing — they are still being read.
fn catalog(inner: &mut Inner) -> Result<&Catalog, String> {
    inner.catalog.as_ref().ok_or_else(|| {
        inner.catalog_error.clone().unwrap_or_else(|| {
            "the printer and material profiles are still being read — try again in a moment".into()
        })
    })
}

#[tauri::command]
fn app_status(state: State<AppState>) -> Result<Status, String> {
    let mut inner = lock(&state)?;
    let loaded = catalog(&mut inner).is_ok();
    let resources_message = match (&inner.catalog, &inner.catalog_error) {
        (Some(_), _) => "Printer resources are loaded.".to_string(),
        (None, Some(error)) => error.clone(),
        // Not loaded and not failed: the background read is still running.
        (None, None) => "Reading the printer and material profiles…".to_string(),
    };
    let printer_count = inner
        .catalog
        .as_ref()
        .map(|c| c.printers().len())
        .unwrap_or(0);
    let engine_ok = inner.engine.available();
    Ok(Status {
        resources_ok: loaded,
        resources_message,
        engine_path: inner.engine.cli.program.display().to_string(),
        engine_lib_dir: inner
            .engine
            .cli
            .lib_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        engine_ok,
        engine_message: inner.engine_message.clone(),
        engine_backend: inner.engine.backend.as_str().to_string(),
        printer_count,
        demo_model: std::env::var("SHALE_DEMO_MODEL").ok(),
        demo: std::env::var("SHALE_DEMO").ok(),
        catalog_loading: inner.catalog.is_none() && inner.catalog_error.is_none(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[tauri::command]
fn set_engine(
    state: State<AppState>,
    path: String,
    lib_dir: Option<String>,
    backend: Option<String>,
) -> Result<bool, String> {
    let mut inner = lock(&state)?;
    inner.engine.cli.program = PathBuf::from(path);
    inner.engine.cli.lib_dir = lib_dir.filter(|value| !value.is_empty()).map(PathBuf::from);
    if let Some(backend) = backend {
        inner.engine.backend = EngineBackend::parse(&backend);
    }
    match inner.engine.cli.probe() {
        Ok(()) => {
            inner.engine_message = format!(
                "CuraEngine answered at {}",
                inner.engine.cli.program.display()
            );
            Ok(true)
        }
        Err(err) => {
            inner.engine_message = err;
            Ok(false)
        }
    }
}

#[tauri::command]
fn cancel_import(state: State<AppState>) -> Result<(), String> {
    let inner = lock(&state)?;
    inner.cancel_import.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn list_printers(state: State<AppState>) -> Result<Vec<PrinterInfo>, String> {
    let mut inner = lock(&state)?;
    Ok(catalog(&mut inner)?.printers())
}

#[tauri::command]
fn list_variants(state: State<AppState>, printer_id: String) -> Result<Vec<VariantInfo>, String> {
    let mut inner = lock(&state)?;
    let catalog = catalog(&mut inner)?;
    Ok(catalog.variants_for(&printer_id))
}

#[tauri::command]
fn list_qualities(state: State<AppState>, printer_id: String) -> Result<Vec<QualityInfo>, String> {
    let mut inner = lock(&state)?;
    let catalog = catalog(&mut inner)?;
    Ok(catalog.qualities_for(&printer_id))
}

#[tauri::command]
fn list_materials(state: State<AppState>, printer_id: String) -> Result<Vec<MaterialInfo>, String> {
    let mut inner = lock(&state)?;
    let catalog = catalog(&mut inner)?;
    let diameter = catalog
        .printer(&printer_id)
        .and_then(|_| {
            catalog
                .resolve(&shale_core::ResolveRequest {
                    printer_id: printer_id.clone(),
                    variant_name: None,
                    quality_type: None,
                    material_id: None,
                    intent_category: None,
                    settings: UserSettings::default(),
                })
                .ok()
        })
        .map(|resolved| resolved.summary.material_diameter)
        .unwrap_or(1.75);
    // Bambu printers get their own tuned filaments, already narrowed to the
    // machine; Cura printers keep the short generic-first list they had.
    if catalog
        .printers()
        .iter()
        .any(|printer| printer.id == printer_id && printer.source == "bambu")
    {
        return Ok(catalog.materials_for_printer(&printer_id, diameter));
    }
    let mut materials: Vec<MaterialInfo> = catalog
        .materials_for(diameter)
        .into_iter()
        .filter(|material| {
            material.id.starts_with("generic_")
                || material.id.contains("pla")
                    && material.id.chars().filter(|ch| *ch == '_').count() <= 2
        })
        .collect();
    // Prefer the short generic profiles at the top of a long library; Cura
    // ships hundreds of brand filaments whose names sort before `generic_pla`.
    materials.sort_by(|a, b| {
        let generic =
            a.id.starts_with("generic_")
                .cmp(&b.id.starts_with("generic_"));
        generic.reverse().then_with(|| a.id.cmp(&b.id))
    });
    materials.truncate(40);
    Ok(materials)
}

#[tauri::command]
fn printer_bed(
    state: State<AppState>,
    printer_id: String,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
) -> Result<Bed, String> {
    let mut inner = lock(&state)?;
    let catalog = catalog(&mut inner)?;
    let resolved = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id,
            variant_name,
            quality_type,
            material_id,
            intent_category: None,
            settings: UserSettings::default(),
        })
        .map_err(|err| err.to_string())?;
    Ok(bed_from(&resolved))
}

#[tauri::command]
async fn import_model(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    printer_id: Option<String>,
    variant_name: Option<String>,
    quality_type: Option<String>,
    chordal_mm: Option<f64>,
    angular_deg: Option<f64>,
) -> Result<Vec<ModelView>, String> {
    let tolerance = match (chordal_mm, angular_deg) {
        (Some(chordal), Some(angular)) => TessellationTolerance::new(chordal, angular),
        _ => TessellationTolerance::standard(),
    };
    let cancel = {
        let inner = lock(&state)?;
        inner.cancel_import.store(false, Ordering::Relaxed);
        inner.cancel_import.clone()
    };
    let file = PathBuf::from(path);
    let load_path = file.clone();
    let app_progress = app.clone();
    let loaded = tauri::async_runtime::spawn_blocking(move || {
        let source_bytes = fs::read(&load_path)?;
        let objects = shale_core::mesh::load_objects(
            load_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("model"),
            &source_bytes,
            &tolerance,
            &mut |fraction, message| {
                let _ = app_progress.emit(
                    "import-progress",
                    serde_json::json!({ "fraction": fraction, "message": message }),
                );
            },
            &cancel,
        )?;
        Ok::<_, shale_core::Error>((source_bytes, objects))
    })
    .await
    .map_err(|err| format!("import task failed: {err}"))?
    .map_err(|err| err.to_string())?;
    let (source_bytes, objects) = loaded;
    let mut inner = lock(&state)?;
    let placed = printer_id.and_then(|printer_id| {
        catalog(&mut inner)
            .ok()?
            .resolve(&shale_core::ResolveRequest {
                printer_id,
                variant_name,
                quality_type,
                material_id: None,
                intent_category: None,
                settings: UserSettings::default(),
            })
            .ok()
    });
    let file_name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model")
        .to_string();
    // A file of one object is dropped in the middle of the plate. A file of
    // several is an assembly: the parts keep the places the file gave them, or
    // the model would come apart.
    let assembly = objects.len() > 1;
    let mut views = Vec::with_capacity(objects.len());
    for (index, object) in objects.into_iter().enumerate() {
        let transform = if assembly {
            let mut transform = Transform::default();
            drop_to_bed(&object.mesh, &mut transform);
            transform
        } else {
            match &placed {
                Some(resolved) => place_on_bed(
                    &object.mesh,
                    resolved.summary.machine_width,
                    resolved.summary.machine_depth,
                    resolved.summary.machine_center_is_zero,
                ),
                None => {
                    let mut transform = Transform::default();
                    drop_to_bed(&object.mesh, &mut transform);
                    transform
                }
            }
        };
        let mut name = if assembly {
            let wanted = object.name.trim().to_string();
            if wanted.is_empty() || wanted.starts_with("object ") {
                format!("{file_name} part {}", index + 1)
            } else {
                wanted
            }
        } else {
            file_name.clone()
        };
        // Two objects in one file can share a name; the plate cannot. Only the
        // parts on the plate count: a part that was deleted is still held, so
        // that undo can bring it back.
        let mut suffix = 2;
        while inner
            .models
            .iter()
            .any(|model| model.visible && model.name == name)
        {
            name = format!("{} ({suffix})", name.split(" (").next().unwrap_or(&name));
            suffix += 1;
        }
        let triangles = object.mesh.triangles.len();
        let report = ImportReport {
            triangles,
            max_deviation_mm: None,
            max_turning_deg: None,
            milliseconds: 0,
            source: "3mf".to_string(),
            chordal_mm: None,
            angular_deg: None,
        };
        let id = inner.next_id;
        inner.next_id += 1;
        let settings = std::collections::BTreeMap::new();
        let view = view_of(id, &name, &transform, &report, &settings, None);
        inner.models.push(LoadedModel {
            id,
            name,
            mesh: object.mesh,
            transform,
            report,
            source_bytes: source_bytes.clone(),
            visible: true,
            settings: std::collections::BTreeMap::new(),
            slot: None,
        });
        views.push(view);
    }
    if views.is_empty() {
        return Err("That file held no geometry".to_string());
    }
    Ok(views)
}

/// Fill the plate with copies of one part, the way Bambu Studio's "Fill plate"
/// does: step the part's own footprint across the bed with a gap between copies
/// and skip any place that is already taken. The first copy keeps the model it
/// came from; the rest are new parts with their own ids, so each can be moved,
/// painted or deleted on its own.
#[tauri::command]
fn fill_plate(
    state: State<AppState>,
    id: u32,
    gap: Option<f64>,
    bed_width: Option<f64>,
    bed_depth: Option<f64>,
    center_is_zero: Option<bool>,
) -> Result<Vec<ModelView>, String> {
    let mut inner = lock(&state)?;
    // Copied out of the borrow up front: the loop below adds to `inner.models`.
    let (source_name, source_mesh, source_transform, source_report, source_bytes) = {
        let source = inner
            .models
            .iter()
            .find(|model| model.id == id && model.visible)
            .ok_or_else(|| "that part is not on the plate".to_string())?;
        (
            source.name.clone(),
            source.mesh.clone(),
            source.transform.clone(),
            source.report.clone(),
            source.source_bytes.clone(),
        )
    };
    let (min, max) = source_mesh.bounds();
    let scale = source_transform.scale;
    let width_mm = (max[0] - min[0]) as f64 * scale[0];
    let depth_mm = (max[1] - min[1]) as f64 * scale[1];
    let gap = gap.unwrap_or(4.0).max(0.0);
    let step_x = width_mm + gap;
    let step_y = depth_mm + gap;
    if step_x <= 0.0 || step_y <= 0.0 {
        return Err("that part has no footprint to repeat".to_string());
    }
    let bed_width = bed_width.unwrap_or(220.0);
    let bed_depth = bed_depth.unwrap_or(220.0);
    let center_zero = center_is_zero.unwrap_or(false);
    let (low_x, high_x) = if center_zero {
        (-bed_width / 2.0, bed_width / 2.0)
    } else {
        (0.0, bed_width)
    };
    let (low_y, high_y) = if center_zero {
        (-bed_depth / 2.0, bed_depth / 2.0)
    } else {
        (0.0, bed_depth)
    };

    // What is already on the plate, in absolute plate coordinates.
    let taken: Vec<([f64; 2], [f64; 2])> = inner
        .models
        .iter()
        .filter(|model| model.visible && model.id != id)
        .map(|model| {
            let (low, high) = model.mesh.bounds();
            let shift = &model.transform.translation;
            (
                [
                    low[0] as f64 * model.transform.scale[0] + shift[0],
                    low[1] as f64 * model.transform.scale[1] + shift[1],
                ],
                [
                    high[0] as f64 * model.transform.scale[0] + shift[0],
                    high[1] as f64 * model.transform.scale[1] + shift[1],
                ],
            )
        })
        .collect();

    let base = source_transform;
    // The walk starts in the plate's own low corner rather than at the part:
    // "fill the plate" means the whole plate, and the original's cell is simply
    // skipped where it falls.
    let mut placed: Vec<([f64; 2], [f64; 2])> = Vec::new();
    let mut views = Vec::new();
    let mut copies: Vec<LoadedModel> = Vec::new();
    let mut y = low_y + depth_mm / 2.0;
    'rows: while y + depth_mm / 2.0 <= high_y + 1e-6 {
        let mut x = low_x + width_mm / 2.0;
        while x + width_mm / 2.0 <= high_x + 1e-6 {
            let cell_low = [x - width_mm / 2.0, y - depth_mm / 2.0];
            let cell_high = [x + width_mm / 2.0, y + depth_mm / 2.0];
            let here = base.translation[0];
            let there = base.translation[1];
            let holds_the_original =
                (here - x).abs() < step_x / 2.0 && (there - y).abs() < step_y / 2.0;
            let blocked = holds_the_original
                || taken
                    .iter()
                    .chain(placed.iter())
                    .any(|(other_low, other_high)| {
                        cell_low[0] < other_high[0] + gap / 2.0
                            && cell_high[0] > other_low[0] - gap / 2.0
                            && cell_low[1] < other_high[1] + gap / 2.0
                            && cell_high[1] > other_low[1] - gap / 2.0
                    });
            if !blocked {
                let mut transform = base.clone();
                transform.translation[0] = x;
                transform.translation[1] = y;
                let name = format!("{source_name} · {}", views.len() + 2);
                let report = ImportReport {
                    triangles: source_report.triangles,
                    max_deviation_mm: None,
                    max_turning_deg: None,
                    milliseconds: 0,
                    source: "copy".to_string(),
                    chordal_mm: None,
                    angular_deg: None,
                };
                let new_id = inner.next_id + copies.len() as u32;
                let settings = std::collections::BTreeMap::new();
                views.push(view_of(new_id, &name, &transform, &report, &settings, None));
                copies.push(LoadedModel {
                    id: new_id,
                    name,
                    mesh: source_mesh.clone(),
                    transform,
                    report,
                    source_bytes: source_bytes.clone(),
                    visible: true,
                    settings: std::collections::BTreeMap::new(),
                    slot: None,
                });
                placed.push((cell_low, cell_high));
                if views.len() >= 40 {
                    break 'rows;
                }
            }
            x += step_x;
        }
        y += step_y;
    }
    if views.is_empty() {
        return Err("nothing else fits on this plate".to_string());
    }
    inner.next_id += copies.len() as u32;
    inner.models.extend(copies);
    inner.toolpath = None;
    inner.gcode_path = None;
    Ok(views)
}

/// A solid put on the plate without a file.
///
/// Bambu Studio's right-click menu has "Add primitive"; it is the fastest way to
/// get a first-layer pad or a hole-clearance cylinder onto the bed. The part
/// lands on a free patch rather than on top of whatever is already there, so
/// adding four cubes in a row gives four cubes and not one.
#[tauri::command]
fn add_primitive(
    state: State<AppState>,
    kind: String,
    size: Option<f64>,
    height: Option<f64>,
    bed_width: Option<f64>,
    bed_depth: Option<f64>,
    center_is_zero: Option<bool>,
) -> Result<ModelView, String> {
    let mut inner = lock(&state)?;
    let size = size.unwrap_or(20.0).clamp(1.0, 400.0);
    let height = height.unwrap_or(size).clamp(0.2, 400.0);
    let mesh = match kind.as_str() {
        "cube" | "box" => shale_core::primitives::box_solid(size, size, height),
        "cylinder" => shale_core::primitives::cylinder(size / 2.0, height),
        "cone" => shale_core::primitives::cone(size / 2.0, height),
        "sphere" => shale_core::primitives::sphere(size / 2.0),
        other => return Err(format!("no primitive called {other}")),
    };
    // A solid built here has no file behind it, and a plate cannot be saved
    // without one. Write its own STL now: it is the same bytes an import would
    // have brought, so saving, reopening and exporting all behave alike.
    let source_bytes = shale_core::exchange::to_stl(&mesh);
    // The window already knows the bed it is drawing, so it says so rather than
    // making this command resolve a printer again.
    let width = bed_width.unwrap_or(220.0);
    let depth = bed_depth.unwrap_or(220.0);
    let centre_zero = center_is_zero.unwrap_or(false);
    let mut transform = place_on_bed(&mesh, width, depth, centre_zero);
    let (min, max) = mesh.bounds();
    let foot = ((max[0] - min[0]) as f64, (max[1] - min[1]) as f64);

    // Walk a coarse grid across the bed and take the first patch that touches
    // nothing already on the plate.
    let taken: Vec<([f64; 2], [f64; 2])> = inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| {
            let (low, high) = model.mesh.bounds();
            let shift = &model.transform.translation;
            (
                [low[0] as f64 + shift[0], low[1] as f64 + shift[1]],
                [high[0] as f64 + shift[0], high[1] as f64 + shift[1]],
            )
        })
        .collect();
    let gap = 4.0;
    let step_x = foot.0 + gap;
    let step_y = foot.1 + gap;
    'search: for row in 0..12 {
        for column in 0..12 {
            let mut candidate = transform.clone();
            candidate.translation[0] = transform.translation[0] + column as f64 * step_x;
            candidate.translation[1] = transform.translation[1] - row as f64 * step_y;
            let low = [
                min[0] as f64 + candidate.translation[0],
                min[1] as f64 + candidate.translation[1],
            ];
            let high = [
                max[0] as f64 + candidate.translation[0],
                max[1] as f64 + candidate.translation[1],
            ];
            let hits = taken.iter().any(|(other_low, other_high)| {
                low[0] < other_high[0] + gap / 2.0
                    && high[0] > other_low[0] - gap / 2.0
                    && low[1] < other_high[1] + gap / 2.0
                    && high[1] > other_low[1] - gap / 2.0
            });
            if !hits {
                transform = candidate;
                break 'search;
            }
        }
    }

    let report = ImportReport {
        triangles: mesh.triangles.len(),
        max_deviation_mm: None,
        max_turning_deg: None,
        milliseconds: 0,
        source: "primitive".to_string(),
        chordal_mm: None,
        angular_deg: None,
    };
    let name = format!("{kind} {size:.0} mm");
    let id = inner.next_id;
    inner.next_id += 1;
    let settings = std::collections::BTreeMap::new();
    let view = view_of(id, &name, &transform, &report, &settings, None);
    inner.models.push(LoadedModel {
        id,
        name,
        mesh,
        transform,
        report,
        source_bytes,
        visible: true,
        settings: std::collections::BTreeMap::new(),
        slot: None,
    });
    inner.toolpath = None;
    inner.gcode_path = None;
    Ok(view)
}

/// One band of a calibration print, as the window sees it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct CalibrationBand {
    index: usize,
    z_mm: f64,
    /// °C, percent of flow, or K of pressure advance, depending on `kind`.
    value: f64,
}

#[derive(serde::Serialize)]
struct TowerBuilt {
    model: ModelView,
    bands: Vec<CalibrationBand>,
    band_height_mm: f64,
    /// `temperature`, `flow` or `pressure` — which line the step writes.
    kind: String,
    /// `C`, `%` or `K`, so the panel can write the numbers properly.
    unit: String,
}

#[derive(serde::Serialize)]
struct CalibrationStep {
    band: usize,
    layer: usize,
    z_mm: f64,
    value: f64,
}

#[derive(serde::Serialize)]
struct CalibrationApplied {
    path: String,
    bytes: usize,
    bands: Vec<CalibrationBand>,
    injections: Vec<CalibrationStep>,
}

/// Where the middle of the plate is, for a print Shale makes itself.
fn plate_centre(inner: &mut Inner, printer_id: Option<String>) -> [f64; 2] {
    match printer_id {
        Some(printer_id) => catalog(inner)
            .ok()
            .and_then(|catalog| {
                catalog
                    .resolve(&shale_core::ResolveRequest {
                        printer_id,
                        variant_name: None,
                        quality_type: None,
                        material_id: None,
                        intent_category: None,
                        settings: UserSettings::default(),
                    })
                    .ok()
            })
            .map(|resolved| {
                [
                    resolved.summary.machine_width / 2.0,
                    resolved.summary.machine_depth / 2.0,
                ]
            })
            .unwrap_or([128.0, 128.0]),
        None => [128.0, 128.0],
    }
}

/// Put a calibration print on the plate and hand the window its bands.
fn build_calibration(
    inner: &mut Inner,
    name: String,
    tower: shale_core::calibrate::Tower,
    step: shale_core::calibrate::Step,
) -> TowerBuilt {
    let report = ImportReport {
        triangles: tower.mesh.triangles.len(),
        max_deviation_mm: None,
        max_turning_deg: None,
        milliseconds: 0,
        source: "calibration".to_string(),
        chordal_mm: None,
        angular_deg: None,
    };
    let source_bytes = shale_core::exchange::to_stl(&tower.mesh);
    let id = inner.next_id;
    inner.next_id += 1;
    let transform = Transform::default();
    let settings = std::collections::BTreeMap::new();
    let view = view_of(id, &name, &transform, &report, &settings, None);
    inner.models.push(LoadedModel {
        id,
        name,
        mesh: tower.mesh,
        transform,
        report,
        source_bytes,
        visible: true,
        settings: std::collections::BTreeMap::new(),
        slot: None,
    });
    TowerBuilt {
        model: view,
        bands: tower
            .bands
            .iter()
            .map(|band| CalibrationBand {
                index: band.index,
                z_mm: band.z_mm,
                value: band.value,
            })
            .collect(),
        band_height_mm: tower.band_height_mm,
        kind: step.name().to_string(),
        unit: step.unit().to_string(),
    }
}

/// Build a temperature tower and put it on the plate.
///
/// The tower is geometry Shale makes itself, so the only thing the engine sees
/// is an ordinary STL: five blocks, ten millimetres each, alternating 20 and
/// 26 mm so every band meets the same 3 mm overhang.
#[tauri::command]
fn calibration_tower(
    state: State<AppState>,
    printer_id: Option<String>,
    bands: Option<usize>,
    start_c: Option<f64>,
    step_c: Option<f64>,
    band_height_mm: Option<f64>,
) -> Result<TowerBuilt, String> {
    let mut inner = lock(&state)?;
    let centre = plate_centre(&mut inner, printer_id);
    let bands = bands.unwrap_or(5).clamp(2, 10);
    let start_c = start_c.unwrap_or(230.0);
    let step_c = step_c.unwrap_or(5.0);
    if !shale_core::calibrate::cooling_step(start_c, step_c, bands) {
        return Err(format!(
            "{bands} bands of {step_c} °C from {start_c} °C would end at or below zero; ask for fewer bands or a smaller step"
        ));
    }
    let height = band_height_mm.unwrap_or(shale_core::calibrate::BAND_HEIGHT_MM);
    let tower = shale_core::calibrate::temperature_tower(centre, bands, height, start_c, step_c);
    let name = format!("temperature tower ({bands} bands)");
    Ok(build_calibration(
        &mut inner,
        name,
        tower,
        shale_core::calibrate::Step::Temperature,
    ))
}

/// Build a single-wall tube whose flow steps down as it rises.
///
/// Flow is measured, not judged: at the right percentage the wall calipers at
/// the line width. Five bands of 2.5 % from 105 % down to 95 % covers the usual
/// error in either direction.
#[tauri::command]
fn flow_coupon(
    state: State<AppState>,
    printer_id: Option<String>,
    bands: Option<usize>,
    start_pct: Option<f64>,
    step_pct: Option<f64>,
    band_height_mm: Option<f64>,
) -> Result<TowerBuilt, String> {
    let mut inner = lock(&state)?;
    let centre = plate_centre(&mut inner, printer_id);
    let bands = bands.unwrap_or(5).clamp(2, 10);
    let start_pct = start_pct.unwrap_or(105.0);
    let step_pct = step_pct.unwrap_or(2.5);
    if step_pct <= 0.0 {
        return Err("The flow coupon has to step downwards".to_string());
    }
    let lowest = start_pct - step_pct * (bands - 1) as f64;
    if lowest <= 20.0 || start_pct > 200.0 {
        return Err(format!(
            "{bands} bands of {step_pct} % from {start_pct} % would end at {lowest} %; ask for fewer bands or a smaller step"
        ));
    }
    let height = band_height_mm.unwrap_or(shale_core::calibrate::BAND_HEIGHT_MM);
    let coupon = shale_core::calibrate::flow_coupon(centre, bands, height, start_pct, step_pct);
    let name = format!("flow coupon ({bands} bands)");
    Ok(build_calibration(
        &mut inner,
        name,
        coupon,
        shale_core::calibrate::Step::Flow,
    ))
}

/// Build the alternating tower again, this time stepping pressure advance.
#[tauri::command]
fn pressure_tower(
    state: State<AppState>,
    printer_id: Option<String>,
    bands: Option<usize>,
    start_k: Option<f64>,
    step_k: Option<f64>,
    band_height_mm: Option<f64>,
) -> Result<TowerBuilt, String> {
    let mut inner = lock(&state)?;
    let centre = plate_centre(&mut inner, printer_id);
    let bands = bands.unwrap_or(5).clamp(2, 10);
    let start_k = start_k.unwrap_or(0.0);
    let step_k = step_k.unwrap_or(0.01);
    if step_k <= 0.0 {
        return Err(
            "Pressure advance has to step upwards, or there is nothing to read".to_string(),
        );
    }
    let highest = start_k + step_k * (bands - 1) as f64;
    if start_k < 0.0 || highest > 1.0 {
        return Err(format!(
            "{bands} bands of {step_k} K from {start_k} K would reach {highest} K; pressure advance above 1 is not a setting"
        ));
    }
    let height = band_height_mm.unwrap_or(shale_core::calibrate::BAND_HEIGHT_MM);
    let tower = shale_core::calibrate::pressure_tower(centre, bands, height, start_k, step_k);
    let name = format!("pressure tower ({bands} bands)");
    Ok(build_calibration(
        &mut inner,
        name,
        tower,
        shale_core::calibrate::Step::PressureAdvance,
    ))
}

/// Step the slice that is already on screen, one line per band.
///
/// `kind` says which line: `temperature` (the default), `flow` or `pressure`.
#[tauri::command]
fn calibrate_slice(
    state: State<AppState>,
    bands: Vec<CalibrationBand>,
    kind: Option<String>,
) -> Result<CalibrationApplied, String> {
    if bands.is_empty() {
        return Err("There is no calibration plan to apply".to_string());
    }
    let step = match kind.as_deref() {
        None | Some("temperature") | Some("temperature-tower") => {
            shale_core::calibrate::Step::Temperature
        }
        Some("flow") | Some("flow-coupon") => shale_core::calibrate::Step::Flow,
        Some("pressure") | Some("pressure-tower") => shale_core::calibrate::Step::PressureAdvance,
        Some(other) => return Err(format!("There is no calibration called {other}")),
    };
    let path = {
        let inner = lock(&state)?;
        inner
            .gcode_path
            .clone()
            .ok_or_else(|| "Slice the plate before stepping it".to_string())?
    };
    let gcode =
        fs::read_to_string(&path).map_err(|err| format!("could not read the slice: {err}"))?;
    let plan: Vec<shale_core::calibrate::Band> = bands
        .iter()
        .map(|band| shale_core::calibrate::Band {
            index: band.index,
            z_mm: band.z_mm,
            value: band.value,
        })
        .collect();
    let (rewritten, injected) = shale_core::calibrate::step_bands(&gcode, &plan, step);
    if injected.is_empty() {
        return Err("The slice has no layer at any band height".to_string());
    }
    fs::write(&path, rewritten.as_bytes())
        .map_err(|err| format!("could not write the slice: {err}"))?;
    Ok(CalibrationApplied {
        path: path.to_string_lossy().to_string(),
        bytes: rewritten.len(),
        bands,
        injections: injected
            .into_iter()
            .map(|injected| CalibrationStep {
                band: injected.band,
                layer: injected.layer,
                z_mm: injected.z_mm,
                value: injected.value,
            })
            .collect(),
    })
}

#[tauri::command]
fn model_geometry(
    state: State<AppState>,
    id: u32,
    max_triangles: Option<usize>,
) -> Result<tauri::ipc::Response, String> {
    let inner = lock(&state)?;
    let model = inner
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    let budget = max_triangles.unwrap_or(2_000_000);
    let positions = model.mesh.preview_positions(budget);
    let mut bytes = Vec::with_capacity(4 + positions.len() * 4);
    bytes.extend_from_slice(&(positions.len() as u32).to_le_bytes());
    for value in positions {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    // Geometry goes over the IPC as bytes: a million triangles is 36 MB of
    // floats, which as JSON costs a long stringify, a long parse, and a
    // truncated payload on the largest parts.
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
fn duplicate_model(state: State<AppState>, id: u32) -> Result<ModelView, String> {
    let mut inner = lock(&state)?;
    let index = inner
        .models
        .iter()
        .position(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    let new_id = inner.next_id;
    inner.next_id += 1;
    let mut transform = inner.models[index].transform.clone();
    // Offset the copy along the bed's X axis so it does not sit inside the
    // original, then put it back on the plate where the original stands.
    let tall = inner.models[index].transform.translation[2];
    transform.translation[0] += 20.0;
    transform.translation[2] = tall;
    let name = format!("{} copy", inner.models[index].name);
    let view = view_of(
        new_id,
        &name,
        &transform,
        &inner.models[index].report,
        &inner.models[index].settings,
        inner.models[index].slot.as_ref(),
    );
    let model = LoadedModel {
        id: new_id,
        name,
        mesh: inner.models[index].mesh.clone(),
        transform,
        report: inner.models[index].report.clone(),
        source_bytes: inner.models[index].source_bytes.clone(),
        visible: true,
        settings: inner.models[index].settings.clone(),
        slot: inner.models[index].slot,
    };
    inner.models.push(model);
    Ok(view)
}

#[tauri::command]
fn update_transform(state: State<AppState>, id: u32, transform: Transform) -> Result<(), String> {
    let mut inner = lock(&state)?;
    {
        let model = inner
            .models
            .iter_mut()
            .find(|model| model.id == id)
            .ok_or_else(|| "That model is no longer loaded".to_string())?;
        model.transform = transform;
    }
    inner.support = None;
    inner.support_model_id = None;
    Ok(())
}

/// Stand the part on whichever face needs the least support, in the place it
/// already occupies. The scoring lives in `shale_core::orient`; this only moves
/// the result onto the model and clears the stale slice.
#[tauri::command]
fn auto_orient(state: State<AppState>, id: u32) -> Result<Transform, String> {
    let mut inner = lock(&state)?;
    // Cura's `support_angle` default: a face steeper than this from vertical
    // is what the slicer would hold up, so it is what the search scores.
    let angle = shale_core::supports::DEFAULT_SUPPORT_ANGLE;
    {
        let model = inner
            .models
            .iter_mut()
            .find(|model| model.id == id)
            .ok_or_else(|| "That model is no longer loaded".to_string())?;
        model.transform = shale_core::orient::orient(&model.mesh, &model.transform, angle);
        let transform = model.transform.clone();
        inner.support = None;
        inner.support_model_id = None;
        Ok(transform)
    }
}

/// Rest the face the user clicked on the plate.
///
/// The viewport knows the normal of the triangle under the cursor in world
/// space; the core turns the part so that normal points at the bed, keeps it
/// where it stood in plan and drops it. Brute-force orientation picks the
/// orientation that needs least support; this is the one the person asked for.
#[tauri::command]
fn place_on_face(state: State<AppState>, id: u32, normal: [f64; 3]) -> Result<Transform, String> {
    let mut inner = lock(&state)?;
    let model = inner
        .models
        .iter_mut()
        .find(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    model.transform = shale_core::orient::face_rest(&model.mesh, &model.transform, normal);
    let transform = model.transform.clone();
    inner.support = None;
    inner.support_model_id = None;
    Ok(transform)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MeshHealth {
    triangles: usize,
    open_edges: usize,
    volume_mm3: f64,
}

/// What the part is, measured: how many triangles, how much material, and
/// whether its surface closes.
///
/// A slicer assumes a closed surface. Zero open edges means the part came in
/// clean; a cut leaves a seam of T-junctions where the BSP split one polygon
/// without splitting its neighbour, and that shows as a non-zero count without
/// being a hole — so the number is shown, and the panel does not call it an
/// error.
#[tauri::command]
fn mesh_health(state: State<AppState>, id: u32) -> Result<MeshHealth, String> {
    let inner = lock(&state)?;
    let model = inner
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    let world = shale_core::mesh::bake(&model.mesh, &model.transform);
    Ok(MeshHealth {
        triangles: world.triangles.len(),
        open_edges: shale_core::csg::open_edges(&world),
        volume_mm3: shale_core::cut::volume(&world),
    })
}

/// Where a click on a part should land once it is pulled onto the nearest
/// vertex. `None` means the click was in the middle of a face and is better
/// left where the ray put it.
#[tauri::command]
fn snap_measure(
    state: State<AppState>,
    id: u32,
    point: [f64; 3],
) -> Result<Option<[f64; 3]>, String> {
    let inner = lock(&state)?;
    let model = inner
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    Ok(shale_core::measure::snap(
        &model.mesh,
        &model.transform,
        point,
        shale_core::measure::SNAP_MM,
    ))
}

/// The number the ruler shows: the distance between two picked points and how
/// much of it lies along each axis. The frontend could do this arithmetic, but
/// then the number on screen would not be the number the tests check.
#[tauri::command]
fn measure_span(a: [f64; 3], b: [f64; 3]) -> shale_core::measure::Span {
    shale_core::measure::span(a, b)
}

/// Split a part with a plane through it, the way Bambu's *Cut* tool does.
///
/// `axis` is the plane's normal (0 = X, 1 = Y, 2 = Z) and `offset` slides the
/// plane from the part's own centre. Both halves arrive in world space with an
/// identity transform, because cutting is geometry: nothing is left to rotate.
/// The mesh boolean is only proven on small meshes, and the limit is measured.
///
/// `csg` is a BSP tree, and a BSP tree is only as good as its split planes.
/// Choosing the plane fixed the concave case: a plate with a bore (2 554
/// triangles) booleans in 57 ms and the 1946-triangle bracket in 0.01 s. It does
/// not fix a **convex** body, where every candidate plane is a face plane and
/// each node peels one face, so the tree is a chain — 7 820 triangles come out
/// 4 264 deep and cost 5.1 s to build, and the cost grows as the square of the
/// count (see `boolean_timings_at_eight_thirty_and_eighty_thousand`). A part
/// over this size would sit on a spinner for minutes rather than fail, so the
/// guard is a word instead. It is a limit on the kernel, not on the printer:
/// raising it needs an iterative build, not a bigger stack.
/// The largest part the mesh engine is handed. It cuts 32 448 triangles in a
/// tenth of a second, so this is a guard against a mistake in the plate
/// rather than a polygon budget: a boolean asked for in a loop over a
/// thousand copies should say so, not hang the window.
const BOOLEAN_CAP: usize = shale_core::csg::MANIFOLD_CAP;

/// The sentence a boolean refusal earns, in the same words wherever it is asked.
///
/// The cap is about **time**, not memory: the BSP boolean finishes 7 800
/// triangles in 5.1 s and 32 500 in 215.6 s, because a convex body turns the
/// tree into a chain of facet planes and every fragment is re-split by each one.
/// Saying "in memory" was wrong and saying "only proven on small parts" told the
/// user nothing they could act on. This says the number, the reason and what
/// still works.
fn boolean_refusal(triangles: usize, alternative: &str) -> String {
    format!(
        "A mesh boolean on {triangles} triangles is more than Shale will attempt at once: \
         the engine is given at most {BOOLEAN_CAP}. That is not about bytes: it is the \
         size at which a boolean is more likely a mistake in the plate than a part. \
         {alternative}"
    )
}

/// BSP clipping recurses once for every polygon it splits, and a real part has
/// enough of them to overflow the two megabytes a thread gets by default — a
/// 1488-triangle bracket with a dovetail overflows `main` and the window dies
/// with no message. Geometry that cuts a mesh runs on a stack that holds it.
fn with_big_stack<T, F>(job: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn_scoped(scope, job)
            .map_err(|err| format!("Could not start the cut: {err}"))?
            .join()
            .map_err(|_| "The cut stopped unexpectedly".to_string())
    })
}

#[tauri::command]
fn cut_model(
    state: State<AppState>,
    id: u32,
    axis: u8,
    offset: f64,
    keep: Option<String>,
    pins: Option<u8>,
    pin_radius: Option<f64>,
    pin_height: Option<f64>,
    dovetail: Option<f64>,
) -> Result<CutResult, String> {
    let mut inner = lock(&state)?;
    let index = inner
        .models
        .iter()
        .position(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    if axis > 2 {
        return Err("A cut plane has three axes".to_string());
    }
    let model = &inner.models[index];
    let world = shale_core::mesh::bake(&model.mesh, &model.transform);
    let (min, max) = shale_core::mesh::bounds(&world);
    let axis = axis as usize;
    let middle = (min[axis] + max[axis]) * 0.5 + offset;
    if middle <= min[axis] + 0.05 || middle >= max[axis] - 0.05 {
        return Err("That plane is outside the part".to_string());
    }
    let mut normal = [0.0, 0.0, 0.0];
    normal[axis] = 1.0;
    let mut point = [0.0, 0.0, 0.0];
    point[axis] = middle;
    let plane = shale_core::cut::Plane::new(point, normal);
    let (pins, dovetail) = (pins.unwrap_or(0), dovetail.unwrap_or(0.0));
    if pins > 0 && dovetail > 0.0 {
        return Err("A cut carries one joint, not two: pegs or a dovetail".to_string());
    }
    if pins > 0 || dovetail > 0.0 {
        let triangles = world.triangles.len();
        if triangles > BOOLEAN_CAP {
            return Err(boolean_refusal(
                triangles,
                "A plain cut, with no pegs or dovetail, still works.",
            ));
        }
    }
    let pin = shale_core::cut::Pin {
        radius: pin_radius.unwrap_or(2.0).clamp(0.4, 12.0),
        height: pin_height.unwrap_or(5.0).clamp(0.6, 40.0),
        clearance: 0.2,
    };
    // What the cut actually did, so the caller can say it: a joint that was
    // asked for and could not be fitted is not the same answer as one that was.
    let mut joint: Option<String> = None;
    let halves = with_big_stack(|| match pins {
        0 if dovetail > 0.0 => {
            // A dovetail is what stops the seam opening: the tab is wider than
            // the way out of its own slot.
            let cut = shale_core::cut::cut_with_dovetail(
                &world,
                &plane,
                shale_core::cut::Dovetail::from_root(dovetail.clamp(2.0, 40.0)),
            );
            joint = cut
                .tab_mm3
                .map(|tab| format!("a dovetail tab of {tab:.0} mm³"));
            cut.halves
        }
        0 => shale_core::cut::cut(&world, &plane),
        // Pins are what makes the cut reversible: the halves come off the
        // plate and go back together in the same place.
        count => {
            joint = Some(match count {
                1 => "1 peg".to_string(),
                _ => format!("{count} pegs"),
            });
            shale_core::cut::cut_with_pins(&world, &plane, pin, count as usize)
        }
    })?;
    let keep = keep.unwrap_or_else(|| "both".to_string());
    let mut pieces: Vec<(&str, TriangleMesh)> = Vec::new();
    if keep != "above" && !halves.below.triangles.is_empty() {
        pieces.push(("lower", halves.below));
    }
    if keep != "below" && !halves.above.triangles.is_empty() {
        pieces.push(("upper", halves.above));
    }
    if pieces.is_empty() {
        return Err("The cut left nothing behind".to_string());
    }
    let name = model.name.clone();
    let report = model.report.clone();
    let settings = model.settings.clone();
    let source_bytes = model.source_bytes.clone();
    // The original leaves the plate, but it does not leave the model list: undo
    // restores by id, and `restore_models` can only bring back what it still
    // has. Dropping it here is how Ctrl+Z after a cut emptied the plate.
    inner.models[index].visible = false;
    let mut views = Vec::new();
    for (suffix, mesh) in pieces {
        let new_id = inner.next_id;
        inner.next_id += 1;
        let name = format!("{name} ({suffix})");
        // The panel reads its triangle count from the report, which still
        // describes the part before it was cut.
        let report = ImportReport {
            triangles: mesh.triangles.len(),
            ..report.clone()
        };
        // Each half keeps its place in plan and is set down on the plate: the
        // upper piece of a horizontal cut would otherwise float above the bed.
        let (low, _) = shale_core::mesh::bounds(&mesh);
        let transform = Transform {
            translation: [0.0, 0.0, -low[2]],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        };
        views.push(view_of(new_id, &name, &transform, &report, &settings, None));
        inner.models.push(LoadedModel {
            id: new_id,
            name,
            mesh,
            transform,
            report: report.clone(),
            source_bytes: source_bytes.clone(),
            visible: true,
            settings: settings.clone(),
            slot: None,
        });
    }
    inner.toolpath = None;
    inner.gcode_path = None;
    Ok(CutResult { views, joint })
}

/// What a cut left behind: the parts, and the joint it managed to carry — or
/// nothing, when the face had no room for the tab that was asked for.
#[derive(Serialize)]
struct CutResult {
    views: Vec<ModelView>,
    joint: Option<String>,
}

/// One part against the others: the boolean Bambu calls a mesh boolean.
/// `mode` is `subtract` (the chosen part keeps only what the others do not
/// cover), `union`, or `common`.
/// What a boolean did, so the notice can say how many parts it actually folded
/// in rather than how many were standing nearby.
#[derive(Serialize)]
struct BooleanResult {
    views: Vec<ModelView>,
    consumed: usize,
}

#[tauri::command]
fn boolean_model(state: State<AppState>, id: u32, mode: String) -> Result<BooleanResult, String> {
    let mut inner = lock(&state)?;
    let index = inner
        .models
        .iter()
        .position(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    if inner.models.len() < 2 {
        return Err("A boolean needs a second part to work against".to_string());
    }
    // Only what is on the plate: a part a previous cut or undo put away is
    // still in the list, and its triangles are not the ones being booleaned.
    let triangles = inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| model.mesh.triangles.len())
        .sum::<usize>();
    if triangles > BOOLEAN_CAP {
        return Err(boolean_refusal(
            triangles,
            "Cut, or boolean fewer parts at a time.",
        ));
    }
    if !matches!(mode.as_str(), "subtract" | "union" | "common") {
        return Err("A boolean is subtract, union, or common".to_string());
    }
    // Both sides are baked into world space: a boolean of two parts that were
    // merely dragged is a boolean of where they are, not of where they were.
    let mut solid =
        shale_core::mesh::bake(&inner.models[index].mesh, &inner.models[index].transform);
    // Only parts that actually touch the chosen one are cutlery. A plate can
    // hold forty parts, and eating the thirty-nine that are nowhere near the
    // one being cut is not a boolean, it is a mistake.
    let (low, high) = shale_core::mesh::bounds(&solid);
    let mut others: Vec<(u32, TriangleMesh)> = Vec::new();
    for (position, model) in inner.models.iter().enumerate() {
        if position == index || !model.visible {
            // A part you cannot see cannot cut: an invisible model taking part
            // in a boolean is a bite out of the plate with no cause on screen.
            continue;
        }
        let world = shale_core::mesh::bake(&model.mesh, &model.transform);
        let (other_low, other_high) = shale_core::mesh::bounds(&world);
        let apart =
            (0..3).any(|axis| other_low[axis] >= high[axis] || other_high[axis] <= low[axis]);
        if !apart {
            others.push((model.id, world));
        }
    }
    if others.is_empty() {
        return Err("No other part overlaps this one, so there is nothing to cut with".to_string());
    }
    for (_, other) in &others {
        let op = match mode.as_str() {
            "subtract" => shale_core::csg::Op::Difference,
            "union" => shale_core::csg::Op::Union,
            _ => shale_core::csg::Op::Intersection,
        };
        // `boolean_within` refuses when the library will not take the mesh
        // and the fallback would take minutes; the sentence says which.
        let next =
            with_big_stack(|| shale_core::csg::boolean_within(&solid, other, op, BOOLEAN_CAP))?
                .ok_or_else(|| {
                    boolean_refusal(
                        solid.triangles.len().max(other.triangles.len()),
                        "Cut, or boolean fewer parts at a time.",
                    )
                })?
                .0;
        // A boolean that empties the part is not a boolean: say so rather than
        // leaving a plate with nothing on it.
        if next.triangles.is_empty() || shale_core::cut::volume(&next).abs() < 0.01 {
            return Err(format!(
                "That {} leaves nothing of this part",
                match mode.as_str() {
                    "subtract" => "cut",
                    "union" => "union",
                    _ => "overlap",
                }
            ));
        }
        solid = next;
    }
    let model = &inner.models[index];
    let name = model.name.clone();
    let report = model.report.clone();
    let settings = model.settings.clone();
    let source_bytes = model.source_bytes.clone();
    // The cutters are cutlery now, not geometry: leaving them on the plate
    // would leave it printed twice. They stay in the model list, invisible, so
    // that undo can put them back — `restore_models` restores by id.
    let spent: Vec<u32> = others.iter().map(|(used, _)| *used).collect();
    for model in inner.models.iter_mut() {
        if model.id == id || spent.contains(&model.id) {
            model.visible = false;
        }
    }
    let (low, _) = shale_core::mesh::bounds(&solid);
    let transform = Transform {
        translation: [0.0, 0.0, -low[2]],
        rotation_deg: [0.0, 0.0, 0.0],
        scale: [1.0, 1.0, 1.0],
    };
    let report = ImportReport {
        triangles: solid.triangles.len(),
        ..report
    };
    let new_id = inner.next_id;
    inner.next_id += 1;
    inner.models.push(LoadedModel {
        id: new_id,
        name,
        mesh: solid,
        transform,
        report,
        source_bytes,
        visible: true,
        settings,
        slot: None,
    });
    inner.toolpath = None;
    inner.gcode_path = None;
    let consumed = others.len();
    // Every part that is still on the plate, not only the one that changed: a
    // list that forgets the parts a boolean did not touch is a list that lies
    // about the plate.
    let views: Vec<ModelView> = inner
        .models
        .iter()
        .map(|model| {
            view_of(
                model.id,
                &model.name,
                &model.transform,
                &model.report,
                &model.settings,
                model.slot.as_ref(),
            )
        })
        .collect();
    Ok(BooleanResult { views, consumed })
}

#[tauri::command]
fn drop_model(state: State<AppState>, id: u32) -> Result<Transform, String> {
    let mut inner = lock(&state)?;
    let model = inner
        .models
        .iter_mut()
        .find(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    drop_to_bed(&model.mesh, &mut model.transform);
    Ok(model.transform.clone())
}

/// The footprint a model covers on the plate, in world millimetres.
fn footprint(mesh: &TriangleMesh, transform: &Transform) -> ([f64; 2], [f64; 2]) {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for vertex in &mesh.vertices {
        let point = shale_core::apply_transform(
            [vertex[0] as f64, vertex[1] as f64, vertex[2] as f64],
            transform,
        );
        min[0] = min[0].min(point[0]);
        min[1] = min[1].min(point[1]);
        max[0] = max[0].max(point[0]);
        max[1] = max[1].max(point[1]);
    }
    (min, max)
}

/// Lay the models out on the plate: biggest footprint first, a shelf at a time,
/// a few millimetres apart. This is the auto-arrange every other slicer has;
/// before it, imported parts landed on top of each other in the middle.
#[tauri::command]
fn arrange_models(
    state: State<AppState>,
    printer_id: Option<String>,
    variant_name: Option<String>,
    quality_type: Option<String>,
) -> Result<Vec<ModelView>, String> {
    let mut inner = lock(&state)?;
    let (width, depth, centre_is_zero) = {
        let resolved = printer_id.and_then(|printer_id| {
            catalog(&mut inner)
                .ok()?
                .resolve(&shale_core::ResolveRequest {
                    printer_id,
                    variant_name,
                    quality_type,
                    material_id: None,
                    intent_category: None,
                    settings: UserSettings::default(),
                })
                .ok()
        });
        match resolved {
            Some(resolved) => (
                resolved.summary.machine_width,
                resolved.summary.machine_depth,
                resolved.summary.machine_center_is_zero,
            ),
            None => (220.0, 220.0, false),
        }
    };
    if !inner.models.iter().any(|model| model.visible) {
        return Ok(Vec::new());
    }

    let origin_x = if centre_is_zero { -width * 0.5 } else { 0.0 };
    let origin_y = if centre_is_zero { -depth * 0.5 } else { 0.0 };
    let sizes: Vec<(f64, f64)> = inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| {
            let (min, max) = footprint(&model.mesh, &model.transform);
            (max[0] - min[0], max[1] - min[1])
        })
        .collect();
    let slots = shale_core::layout::shelves(&sizes, width, depth, 4.0);

    for (model, slot) in inner.models.iter_mut().zip(slots.iter()) {
        let (min, max) = footprint(&model.mesh, &model.transform);
        let centre = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];
        let target = [slot[0] + origin_x, slot[1] + origin_y];
        model.transform.translation[0] += target[0] - centre[0];
        model.transform.translation[1] += target[1] - centre[1];
        drop_to_bed(&model.mesh, &mut model.transform);
    }

    Ok(inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| {
            view_of(
                model.id,
                &model.name,
                &model.transform,
                &model.report,
                &model.settings,
                model.slot.as_ref(),
            )
        })
        .collect())
}

/// One part's place in the scene, as the undo stack remembers it.
#[derive(serde::Deserialize)]
struct ModelState {
    id: u32,
    transform: Transform,
}

/// Put the plate back the way it was: show these parts where they stood and
/// hide everything else. Deleting only hides a part, so undo never has to
/// re-read a file, and a part that was deleted and brought back keeps its
/// geometry, its name and its import report.
#[tauri::command]
fn restore_models(
    state: State<AppState>,
    models: Vec<ModelState>,
) -> Result<Vec<ModelView>, String> {
    let mut inner = lock(&state)?;
    let mut wanted: std::collections::HashMap<u32, Transform> =
        std::collections::HashMap::with_capacity(models.len());
    for model in &models {
        wanted.insert(model.id, model.transform.clone());
    }
    for model in &mut inner.models {
        match wanted.get(&model.id) {
            Some(transform) => {
                model.transform = transform.clone();
                model.visible = true;
            }
            None => model.visible = false,
        }
    }
    inner.support = None;
    inner.support_model_id = None;
    Ok(inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| {
            view_of(
                model.id,
                &model.name,
                &model.transform,
                &model.report,
                &model.settings,
                model.slot.as_ref(),
            )
        })
        .collect())
}

/// Split one part into the bodies it is made of.
///
/// A file made of several shells imports as one part, and one part can only be
/// moved or spooled whole. This hides the whole part and stands each shell in
/// its place as a part of its own, at the coordinates it already stood at, so
/// nothing on the plate moves.
#[tauri::command]
fn split_model(state: State<AppState>, id: u32) -> Result<Vec<ModelView>, String> {
    let mut inner = lock(&state)?;
    let index = inner
        .models
        .iter()
        .position(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    let pieces = shale_core::mesh::split_objects(&inner.models[index].mesh);
    if pieces.len() < 2 {
        return Err("This part is already one piece: there is nothing to split".to_string());
    }

    let name = inner.models[index].name.clone();
    let transform = inner.models[index].transform.clone();
    let source = inner.models[index].report.source.clone();
    let source_bytes = inner.models[index].source_bytes.clone();
    let settings = inner.models[index].settings.clone();
    let slot = inner.models[index].slot;

    // Hidden, not removed: undo brings the whole part back, and the bodies it
    // was split into are hidden in turn by the snapshot that follows.
    inner.models[index].visible = false;

    let total = pieces.len();
    let mut views = Vec::with_capacity(total);
    for (number, piece) in pieces.into_iter().enumerate() {
        let new_id = inner.next_id;
        inner.next_id += 1;
        let piece_name = format!("{} {}", name, number + 1);
        let imported = shale_core::cad::ImportedMesh::from_mesh(piece, &source);
        views.push(view_of(
            new_id,
            &piece_name,
            &transform,
            &imported.report,
            &settings,
            slot.as_ref(),
        ));
        inner.models.push(LoadedModel {
            id: new_id,
            name: piece_name,
            mesh: imported.mesh,
            transform: transform.clone(),
            report: imported.report,
            source_bytes: source_bytes.clone(),
            visible: true,
            settings: settings.clone(),
            slot,
        });
    }
    // The toolpath belonged to the whole part and no longer describes the
    // plate: the next preview slices these bodies, not that one.
    inner.toolpath = None;
    inner.gcode_path = None;
    inner.support = None;
    inner.support_model_id = None;
    Ok(views)
}

#[tauri::command]
fn remove_model(state: State<AppState>, id: u32) -> Result<(), String> {
    let mut inner = lock(&state)?;
    let model = inner
        .models
        .iter_mut()
        .find(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    model.visible = false;
    if inner.support_model_id == Some(id) {
        inner.support = None;
        inner.support_model_id = None;
    }
    Ok(())
}

#[tauri::command]
fn list_instructions(state: State<AppState>) -> Result<Vec<StoredInstruction>, String> {
    let inner = lock(&state)?;
    Ok(stored_instructions(&inner))
}

#[tauri::command]
fn add_instruction(
    state: State<AppState>,
    instruction: CustomInstruction,
) -> Result<StoredInstruction, String> {
    let mut inner = lock(&state)?;
    let id = inner.next_instruction;
    inner.next_instruction += 1;
    let stored = StoredInstruction {
        id,
        summary: instruction.summary(),
        instruction: instruction.clone(),
    };
    inner.instructions.push((id, instruction));
    Ok(stored)
}

#[tauri::command]
fn remove_instruction(state: State<AppState>, id: u32) -> Result<Vec<StoredInstruction>, String> {
    let mut inner = lock(&state)?;
    let before = inner.instructions.len();
    inner.instructions.retain(|(existing, _)| *existing != id);
    if inner.instructions.len() == before {
        return Err(format!("no instruction with id {id}"));
    }
    Ok(stored_instructions(&inner))
}

fn stored_instructions(inner: &Inner) -> Vec<StoredInstruction> {
    inner
        .instructions
        .iter()
        .map(|(id, instruction)| StoredInstruction {
            id: *id,
            summary: instruction.summary(),
            instruction: instruction.clone(),
        })
        .collect()
}

#[derive(Serialize)]
struct SupportView {
    plan: SupportPlan,
    overlays: Vec<SupportOverlay>,
}

fn support_view(plan: &SupportPlan) -> SupportView {
    let drawn = overlays(plan);
    let mut plan = plan.clone();
    for span in &mut plan.spans {
        span.samples.clear();
    }
    SupportView {
        plan,
        overlays: drawn,
    }
}

fn support_params(angle: Option<f64>, bridge: Option<f64>) -> SupportParams {
    let mut params = SupportParams::default();
    if let Some(value) = angle {
        params.overhang_angle_deg = value;
    }
    if let Some(value) = bridge {
        params.max_bridge_mm = value;
    }
    params
}

#[tauri::command]
fn analyse_supports(
    state: State<AppState>,
    id: u32,
    overhang_angle_deg: Option<f64>,
    max_bridge_mm: Option<f64>,
) -> Result<serde_json::Value, String> {
    let inner = lock(&state)?;
    let model = inner
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| "That model is no longer loaded".to_string())?;
    let baked = shale_core::mesh::bake(&model.mesh, &model.transform);
    let analysis = shale_core::analyse(&baked, &support_params(overhang_angle_deg, max_bridge_mm));
    let spans: Vec<_> = analysis
        .spans
        .iter()
        .map(|span| {
            serde_json::json!({
                "kind": span.kind,
                "length_mm": span.length_mm,
                "area_mm2": span.area_mm2,
                "z_mm": span.z_mm,
                "anchor_ends": span.anchor_ends,
            })
        })
        .collect();
    Ok(serde_json::json!({
        "bridges": analysis.count(shale_core::FeatureKind::Bridge),
        "cantilevers": analysis.count(shale_core::FeatureKind::Cantilever),
        "islands": analysis.count(shale_core::FeatureKind::Island),
        "overhangs": analysis.count(shale_core::FeatureKind::Overhang),
        "cosmetic": analysis.count(shale_core::FeatureKind::Cosmetic),
        "spans": spans,
    }))
}

#[tauri::command]
fn propose_supports(
    state: State<AppState>,
    id: u32,
    overhang_angle_deg: Option<f64>,
    max_bridge_mm: Option<f64>,
) -> Result<SupportView, String> {
    let mut inner = lock(&state)?;
    let baked = {
        let model = inner
            .models
            .iter()
            .find(|model| model.id == id)
            .ok_or_else(|| "That model is no longer loaded".to_string())?;
        shale_core::mesh::bake(&model.mesh, &model.transform)
    };
    let plan = propose(&baked, &support_params(overhang_angle_deg, max_bridge_mm));
    let view = support_view(&plan);
    inner.support = Some(plan);
    inner.support_model_id = Some(id);
    Ok(view)
}

#[tauri::command]
fn apply_supports(state: State<AppState>, ids: Vec<u32>) -> Result<SupportView, String> {
    let mut inner = lock(&state)?;
    let plan = inner
        .support
        .as_mut()
        .ok_or_else(|| "Propose supports first".to_string())?;
    set_enabled(plan, Some(&ids));
    Ok(support_view(plan))
}

#[tauri::command]
fn current_support(state: State<AppState>) -> Result<Option<SupportView>, String> {
    let inner = lock(&state)?;
    Ok(inner.support.as_ref().map(support_view))
}

#[tauri::command]
fn compare_supports(
    state: State<AppState>,
    printer_id: String,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    settings: UserSettings,
) -> Result<shale_core::SupportComparison, String> {
    let mut inner = lock(&state)?;
    if inner.slicing {
        return Err("A slice is already running".into());
    }
    let plan = inner
        .support
        .clone()
        .ok_or_else(|| "Propose supports first".to_string())?;
    if inner.models.is_empty() {
        return Err("Import a model first".into());
    }
    let placed: Vec<PlacedModel> = inner
        .models
        .iter()
        .map(|model| PlacedModel {
            name: model.name.clone(),
            mesh: model.mesh.clone(),
            transform: model.transform.clone(),
            settings: model
                .settings
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            slot: None,
        })
        .collect();
    let comparison = {
        let engine = inner.engine.clone();
        let catalog = catalog(&mut inner)?;
        compare_support(
            catalog,
            &engine,
            &printer_id,
            variant_name,
            quality_type,
            material_id,
            settings,
            &placed,
            &plan,
            &mut |_| {},
        )
        .map_err(|err| err.to_string())?
    };
    if let Some(stored) = inner.support.as_mut() {
        stored.comparison = comparison.clone();
    }
    Ok(comparison)
}

/// What is painted on a part, by role.
#[derive(Clone, Default, Debug)]
struct Paint {
    /// Triangles that should grow support under them.
    enforce: Vec<usize>,
    /// Triangles that should keep support out.
    block: Vec<usize>,
}

/// What is painted on a part.
#[derive(Serialize)]
struct PaintView {
    /// How many of the part's triangles ask for support.
    painted: usize,
    /// How many ask for none.
    blocked: usize,
}

/// The patches of underside a part offers, biggest first. One patch is one
/// thing a brush could reach in a single stroke.
#[tauri::command]
fn overhang_patches(
    state: State<AppState>,
    model_id: u32,
) -> Result<Vec<shale_core::paint::Patch>, String> {
    let inner = lock(&state)?;
    let model = inner
        .models
        .iter()
        .find(|model| model.id == model_id)
        .ok_or("That model is no longer loaded")?;
    Ok(shale_core::paint::patches(
        &model.mesh,
        shale_core::paint::PAINT_ANGLE_DEG,
        shale_core::paint::PAINT_REACH_MM,
    ))
}

/// Add triangles to a part's paint, and remember the plate as it was so the
/// stroke can be taken back. Shared by the patch buttons and the brush: a
/// stroke and a patch are the same instruction, differing only in how the
/// triangles were chosen.
fn add_paint(
    inner: &mut Inner,
    model_id: u32,
    triangles: Vec<usize>,
    block: bool,
) -> Result<PaintView, String> {
    let corners = {
        let model = inner
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or("That model is no longer loaded")?;
        model.mesh.triangles.len()
    };
    let before = inner.painted.clone();
    inner.paint_past.push(before);
    if inner.paint_past.len() > PAINT_STEPS {
        inner.paint_past.remove(0);
    }
    inner.paint_future.clear();
    let painting = inner.painted.entry(model_id).or_default();
    // A triangle painted one way is not painted both ways: the later stroke
    // wins, the way a second brush stroke over the first does.
    let (paint, other) = if block {
        (&mut painting.block, &mut painting.enforce)
    } else {
        (&mut painting.enforce, &mut painting.block)
    };
    for triangle in triangles {
        if triangle < corners && !paint.contains(&triangle) {
            paint.push(triangle);
            other.retain(|value| *value != triangle);
        }
    }
    paint.sort_unstable();
    Ok(PaintView {
        painted: painting.enforce.len(),
        blocked: painting.block.len(),
    })
}

/// Paint these triangles as needing support. Painting is additive: a second
/// patch on the same part adds to the first, the way a brush does.
#[tauri::command]
fn paint_support(
    state: State<AppState>,
    model_id: u32,
    triangles: Vec<usize>,
    block: Option<bool>,
) -> Result<PaintView, String> {
    let mut inner = lock(&state)?;
    if inner.slicing {
        return Err("Wait for the slice to finish".into());
    }
    add_paint(&mut inner, model_id, triangles, block.unwrap_or(false))
}

/// Paint with the brush: the frontend sends the point the ray found and the
/// reach, and the triangles are chosen here, next to the mesh. A stroke is a few
/// hundred triangles at most, so sending them one at a time over the boundary
/// would be the wrong direction for the data to flow.
#[tauri::command]
fn paint_brush(
    state: State<AppState>,
    model_id: u32,
    points: Vec<[f64; 3]>,
    radius_mm: f64,
    block: Option<bool>,
) -> Result<PaintView, String> {
    let mut inner = lock(&state)?;
    if inner.slicing {
        return Err("Wait for the slice to finish".into());
    }
    let reach = radius_mm.clamp(0.2, 200.0);
    // The points arrive in the part's own frame, which is the frame the mesh is
    // in: no transform is applied to them on either side. A stroke is a path, so
    // the discs along it are merged into one set before anything is painted —
    // one drag is one edit, and one thing to take back.
    let stroke = {
        let model = inner
            .models
            .iter()
            .find(|model| model.id == model_id)
            .ok_or("That model is no longer loaded")?;
        let mut seen = std::collections::BTreeSet::new();
        for point in points.iter().take(512) {
            for triangle in shale_core::paint::brush_stroke(&model.mesh, *point, reach) {
                seen.insert(triangle);
            }
        }
        seen.into_iter().collect::<Vec<usize>>()
    };
    add_paint(&mut inner, model_id, stroke, block.unwrap_or(false))
}

/// Take the paint off a part. The columns go with it on the next slice.
#[tauri::command]
fn clear_paint(state: State<AppState>, model_id: u32) -> Result<PaintView, String> {
    let mut inner = lock(&state)?;
    if inner.painted.contains_key(&model_id) {
        let before = inner.painted.clone();
        inner.paint_past.push(before);
        if inner.paint_past.len() > PAINT_STEPS {
            inner.paint_past.remove(0);
        }
        inner.paint_future.clear();
        inner.painted.remove(&model_id);
    }
    Ok(PaintView {
        painted: 0,
        blocked: 0,
    })
}

/// How much paint the plate carries in all, which is what a stroke's undo
/// changes and what the paint chip reads back.
const PAINT_STEPS: usize = 60;

fn paint_total(painted: &std::collections::BTreeMap<u32, Paint>) -> usize {
    painted
        .values()
        .map(|paint| paint.enforce.len() + paint.block.len())
        .sum()
}

/// Take the last stroke back. Returns the paint left on the plate.
#[tauri::command]
fn undo_paint(state: State<AppState>) -> Result<PaintView, String> {
    let mut inner = lock(&state)?;
    let Some(previous) = inner.paint_past.pop() else {
        return Err("Nothing painted to take back".into());
    };
    let undone = std::mem::replace(&mut inner.painted, previous);
    inner.paint_future.push(undone);
    if inner.paint_future.len() > PAINT_STEPS {
        inner.paint_future.remove(0);
    }
    Ok(PaintView {
        painted: paint_total(&inner.painted),
        blocked: inner.painted.values().map(|paint| paint.block.len()).sum(),
    })
}

/// Put the last taken-back stroke forward again.
#[tauri::command]
fn redo_paint(state: State<AppState>) -> Result<PaintView, String> {
    let mut inner = lock(&state)?;
    let Some(next) = inner.paint_future.pop() else {
        return Err("No paint to put back".into());
    };
    let replaced = std::mem::replace(&mut inner.painted, next);
    inner.paint_past.push(replaced);
    if inner.paint_past.len() > PAINT_STEPS {
        inner.paint_past.remove(0);
    }
    Ok(PaintView {
        painted: paint_total(&inner.painted),
        blocked: inner.painted.values().map(|paint| paint.block.len()).sum(),
    })
}

/// What the profile itself says, so the settings sheet can show the truth:
/// a field nobody has touched reads its profile value instead of looking blank,
/// and a switch shows the state the printer would actually run.
#[tauri::command]
fn profile_settings(
    state: State<'_, AppState>,
    printer_id: String,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let mut inner = lock(&state)?;
    let catalogue = catalog(&mut inner).map_err(|error| error.to_string())?;
    let resolved = catalogue
        .resolve(&shale_core::ResolveRequest {
            printer_id,
            variant_name,
            quality_type,
            material_id,
            intent_category: None,
            settings: UserSettings::default(),
        })
        .map_err(|error| error.to_string())?;
    Ok(resolved.global.into_iter().collect())
}

/// What a setting is called and what it does, in the words of the printer's own
/// definition. A label alone cannot say what "combing" or "ironing" means, and
/// both Cura and Bambu Studio put the sentence somewhere near the row; the sheet
/// shows it under the setting a hand is on.
#[tauri::command]
fn settings_help(
    state: State<'_, AppState>,
    printer_id: String,
    keys: Vec<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut inner = lock(&state)?;
    let catalogue = catalog(&mut inner).map_err(|error| error.to_string())?;
    Ok(keys
        .into_iter()
        .map(|key| {
            let (label, description) = catalogue.setting_text(&printer_id, &key);
            serde_json::json!({ "key": key, "label": label, "description": description })
        })
        .collect())
}

#[tauri::command]
async fn slice_model(
    app: AppHandle,
    state: State<'_, AppState>,
    printer_id: String,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    ams: Option<Vec<String>>,
    settings: UserSettings,
) -> Result<SliceSummary, String> {
    if std::env::var_os("SHALE_TRACE").is_some() {
        eprintln!("[shale] slice_model extra: {:?}", settings.extra);
    }
    let (catalog_paths, engine, placed, painted, output) = {
        let mut inner = lock(&state)?;
        inner.printer_id = Some(printer_id.clone());
        if inner.slicing {
            return Err("A slice is already running".into());
        }
        if !inner.models.iter().any(|model| model.visible) {
            return Err("Import a model first".into());
        }
        // Wait for the catalogue, then drop the lock: the engine runs outside.
        let _ = catalog(&mut inner)?;
        inner.slicing = true;
        let placed: Vec<PlacedModel> = inner
            .models
            .iter()
            .filter(|model| model.visible)
            .map(|model| PlacedModel {
                name: model.name.clone(),
                mesh: model.mesh.clone(),
                transform: model.transform.clone(),
                settings: model
                    .settings
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                slot: model.slot,
            })
            .collect();
        // The engine is given the parts in plate order, so a painted patch has
        // to name its part by that same order rather than by model id.
        let painted: Vec<shale_core::paint::PaintedPatch> = inner
            .models
            .iter()
            .filter(|model| model.visible)
            .enumerate()
            .flat_map(|(index, model)| {
                let paint = inner.painted.get(&model.id).cloned().unwrap_or_default();
                let mut patches = Vec::new();
                for (triangles, role) in [
                    (paint.enforce, shale_core::paint::PaintRole::Enforce),
                    (paint.block, shale_core::paint::PaintRole::Block),
                ] {
                    if !triangles.is_empty() {
                        patches.push(shale_core::paint::PaintedPatch {
                            model: index,
                            triangles,
                            role,
                        });
                    }
                }
                patches
            })
            .collect();
        let output = std::env::temp_dir().join("shale").join("slice.gcode");
        (
            catalog(&mut inner).ok().map(|c| c.paths.clone()),
            inner.engine.clone(),
            placed,
            painted,
            output,
        )
    };

    let Some(paths) = catalog_paths else {
        let mut inner = lock(&state)?;
        inner.slicing = false;
        return Err("Resources are not loaded".into());
    };
    // Re-open is unnecessary: the catalog is still in state. Slice while not
    // holding the mutex by resolving from a freshly opened catalog would be
    // slow. Hold the catalog reference by cloning the Arc... Catalog is large
    // but Clone is a deep copy of every definition. Avoid that.
    //
    // Resolve and slice under a short lock for the resolve, then release for
    // the engine by splitting the work: resolve now, slice after.
    let (catalog, instructions, support) = {
        let mut inner = lock(&state)?;
        let _ = paths;
        (
            catalog(&mut inner)?.clone(),
            inner
                .instructions
                .iter()
                .map(|(_, instruction)| instruction.clone())
                .collect::<Vec<_>>(),
            inner.support.clone(),
        )
    };
    let mut materials: Vec<Option<String>> = vec![material_id];
    for material in ams.unwrap_or_default() {
        if !material.trim().is_empty() {
            materials.push(Some(material));
        }
    }
    let app_progress = app.clone();
    let resolved_and_models = tauri::async_runtime::spawn_blocking(move || {
        slice(
            &catalog,
            &engine,
            &printer_id,
            variant_name,
            quality_type,
            &materials,
            settings,
            &placed,
            &instructions,
            support.as_ref(),
            &painted,
            output,
            &mut |progress| {
                let layer = progress.layer.as_ref().map(|layer| {
                    serde_json::json!({
                        "index": layer.index,
                        "z": layer.z,
                        "paths": layer.paths.iter().map(|path| serde_json::json!({
                            "kind": path.kind,
                            "points": path.points,
                        })).collect::<Vec<_>>(),
                    })
                });
                let _ = app_progress.emit(
                    "slice-progress",
                    serde_json::json!({
                        "fraction": progress.fraction,
                        "message": progress.message,
                        "layer": layer,
                    }),
                );
            },
        )
    })
    .await
    .map_err(|err| {
        if let Ok(mut inner) = lock(&state) {
            inner.slicing = false;
        }
        format!("slice task failed: {err}")
    })?;

    let mut inner = lock(&state)?;
    inner.slicing = false;
    let outcome = resolved_and_models.map_err(|err| err.to_string())?;
    // The engine reports its duration two ways: a figure on the outcome, and
    // the `;TIME:` line `annotate_header` measured from the file's own
    // `;TIME_ELAPSED:` markers. When the first is missing the second is the
    // truth, and the file is what the person will actually print.
    let facts = shale_core::gcode::header_facts(&read_head(&outcome.gcode_path, 64 * 1024));
    let summary = SliceSummary {
        layer_count: outcome.toolpath.layers.len(),
        layers: outcome
            .toolpath
            .layers
            .iter()
            .map(|layer| LayerMark {
                index: layer.index,
                z: layer.z,
            })
            .collect(),
        print_time_seconds: outcome.print_time_seconds.or(facts.seconds),
        filament_metres: Some(outcome.filament_metres)
            .filter(|metres| *metres > 0.0)
            .or(facts.filament_metres),
        warning_count: outcome.resolved.warning_count,
        gcode_path: outcome.gcode_path.display().to_string(),
        model_ids: inner
            .models
            .iter()
            .map(|model| u64::from(model.id))
            .collect(),
        machine_gcode: outcome.machine_gcode,
    };
    inner.gcode_path = Some(outcome.gcode_path);
    inner.toolpath = Some(outcome.toolpath);
    Ok(summary)
}

#[tauri::command]
fn preview_until(state: State<AppState>, layer: i32) -> Result<Vec<PreviewPath>, String> {
    let inner = lock(&state)?;
    let Some(toolpath) = &inner.toolpath else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    for item in toolpath.layers.iter().filter(|item| item.index <= layer) {
        for polyline in &item.polylines {
            paths.push(PreviewPath {
                kind: polyline.kind.clone(),
                points: polyline.points.clone(),
                feed: polyline.feed,
            });
        }
    }
    Ok(paths)
}

#[tauri::command]
fn loaded_models(state: State<AppState>) -> Result<Vec<ModelView>, String> {
    let inner = lock(&state)?;
    Ok(inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| {
            view_of(
                model.id,
                &model.name,
                &model.transform,
                &model.report,
                &model.settings,
                model.slot.as_ref(),
            )
        })
        .collect())
}

/// The first `limit` bytes of a file as text, cut on a line boundary.
///
/// A sliced file runs to tens of megabytes and the header sits at the top of
/// it: a plate reopened from disk should not read the whole toolpath back to
/// say how long the print takes.
fn read_head(path: &std::path::Path, limit: usize) -> String {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buffer = vec![0u8; limit];
    let read = file.read(&mut buffer).unwrap_or(0);
    buffer.truncate(read);
    let text = String::from_utf8_lossy(&buffer).to_string();
    match text.rfind('\n') {
        Some(index) => text[..index].to_string(),
        None => text,
    }
}

/// The printer link, kept out of `AppState` because it owns a thread and a
/// socket rather than plate data. One at a time: a second connect closes the
/// first, so a wrong host cannot leave a ghost subscription behind.
#[derive(Default)]
struct PrinterHost {
    link: Mutex<Option<shale_host::Link>>,
    sequence: std::sync::atomic::AtomicU64,
    /// The keyring entry this link's certificate pin lives under, and whether
    /// the pin that is there now came from a handshake we watched.
    pin_key: Mutex<String>,
    pin_stored: std::sync::atomic::AtomicBool,
}

impl PrinterHost {
    fn with<T>(&self, f: impl FnOnce(&shale_host::Link) -> Result<T, String>) -> Result<T, String> {
        let slot = self
            .link
            .lock()
            .map_err(|_| "the printer lock is poisoned".to_string())?;
        match slot.as_ref() {
            Some(link) => f(link),
            None => Err("Connect a printer first.".to_string()),
        }
    }
}

/// Host, serial and access code from the environment, so a run can be started
/// with the printer already known without writing a credential into the repo.
#[tauri::command]
fn host_defaults() -> serde_json::Value {
    let env = |key: &str| std::env::var(key).unwrap_or_default();
    serde_json::json!({
        "host": env("SHALE_PRINTER_HOST"),
        "serial": env("SHALE_PRINTER_SERIAL"),
        "code": env("SHALE_PRINTER_CODE"),
        "bedType": if env("SHALE_PRINTER_BED").is_empty() { "textured_plate".to_string() } else { env("SHALE_PRINTER_BED") },
    })
}

#[tauri::command]
fn host_connect(
    config: shale_host::HostConfig,
    state: State<AppState>,
    host: State<PrinterHost>,
) -> Result<shale_host::Status, String> {
    if config.host.trim().is_empty()
        || config.serial.trim().is_empty()
        || config.code.trim().is_empty()
    {
        return Err("A host, a serial and the printer's access code are all needed.".to_string());
    }
    let mut slot = host
        .link
        .lock()
        .map_err(|_| "the printer lock is poisoned".to_string())?;
    if let Some(old) = slot.take() {
        old.close();
    }
    // A printer is identified by its serial; an address is the fallback for a
    // machine that was recorded without one. The key is per printer, so two
    // printers on one network do not overwrite each other's pin.
    let pin_key = format!(
        "pin/{}",
        if config.serial.trim().is_empty() {
            &config.host
        } else {
            &config.serial
        }
    );
    let stored = secrets::get(&pin_key)?;
    let pin = stored.as_deref().and_then(shale_host::parse_hex);
    let link = shale_host::Link::spawn_pinned(config.clone(), pin);
    let status = link.status();
    *slot = Some(link);
    drop(slot);
    *host
        .pin_key
        .lock()
        .map_err(|_| "the pin lock is poisoned".to_string())? = pin_key;
    host.pin_stored
        .store(stored.is_some(), std::sync::atomic::Ordering::Relaxed);
    // Uploads read the same config back out of the plate state.
    lock(&state)?.host_config = Some(config);
    Ok(status)
}

#[tauri::command]
fn host_disconnect(host: State<PrinterHost>) -> Result<(), String> {
    let mut slot = host
        .link
        .lock()
        .map_err(|_| "the printer lock is poisoned".to_string())?;
    if let Some(link) = slot.take() {
        link.close();
    }
    Ok(())
}

/// Forget the machine: drop the link and delete what the vault holds for it.
///
/// A pin outlives a print on purpose — that is what pinning is — but it has to be
/// possible to say "this is not my printer any more" without editing the Windows
/// Credential Manager by hand, which until now was the only way. Both keys the
/// connection writes go: the certificate pin and the access code. The address and
/// serial are the window's (localStorage), so the window clears them.
#[tauri::command]
fn host_forget(link: State<PrinterHost>, serial: String, address: String) -> Result<(), String> {
    {
        let mut slot = link
            .link
            .lock()
            .map_err(|_| "the printer lock is poisoned".to_string())?;
        if let Some(open) = slot.take() {
            open.close();
        }
    }
    let identifier = if serial.trim().is_empty() {
        address.trim()
    } else {
        serial.trim()
    };
    if !identifier.is_empty() {
        let _ = secrets::clear(&format!("pin/{identifier}"));
    }
    let _ = secrets::clear("device.code");
    Ok(())
}

/// Which tools the slice that is about to be sent actually uses. A plate with
/// more than one spool cannot be sent as plain G-code: the machine would print
/// it in one colour. The window asks this before its confirmation so the press
/// can say so instead of the printer doing it silently.
#[tauri::command]
fn slice_tools(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let path = {
        let inner = state
            .0
            .lock()
            .map_err(|_| "the plate lock is poisoned".to_string())?;
        inner.gcode_path.clone()
    };
    let Some(path) = path else {
        return Ok(serde_json::json!({ "tools": [], "changes": 0, "file": null }));
    };
    let text = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let mut tools: Vec<i64> = Vec::new();
    let mut changes = 0usize;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix('T') else {
            continue;
        };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            continue;
        }
        if let Ok(tool) = digits.parse::<i64>() {
            if !tools.contains(&tool) {
                tools.push(tool);
            }
            changes += 1;
        }
    }
    tools.sort_unstable();
    // How much filament each tool takes. A second spool that is needed for a
    // gram is worth knowing about, but it is not the same decision as one that
    // is needed for a hundred.
    let usage: Vec<serde_json::Value> = shale_core::gcode::filament_by_tool(&text)
        .into_iter()
        .map(|(tool, metres)| serde_json::json!({ "tool": tool, "metres": metres }))
        .collect();
    Ok(serde_json::json!({
        "tools": tools,
        "changes": changes,
        "usage": usage,
        "file": std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()),
    }))
}

#[tauri::command]
fn host_status(host: State<PrinterHost>) -> Result<shale_host::Status, String> {
    let slot = host
        .link
        .lock()
        .map_err(|_| "the printer lock is poisoned".to_string())?;
    let status = match slot.as_ref() {
        Some(link) => link.status(),
        None => shale_host::Status::default(),
    };
    drop(slot);

    // Trust on first use, written down: the first status after a connection
    // carries the fingerprint that was accepted, and it is stored against this
    // printer so the next connection can refuse anything else. A mismatch never
    // reaches here — the link does not connect — so this cannot overwrite a pin
    // with the certificate that failed the pin.
    if status.connected
        && !status.fingerprint.is_empty()
        && !host.pin_stored.load(std::sync::atomic::Ordering::Relaxed)
    {
        let key = host
            .pin_key
            .lock()
            .map_err(|_| "the pin lock is poisoned".to_string())?
            .clone();
        if !key.is_empty() {
            secrets::set(&key, &status.fingerprint)?;
            host.pin_stored
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    Ok(status)
}

/// Forget this printer's certificate pin. This is the "trust the new
/// certificate" action: the next connection has nothing to compare against, so
/// it accepts what it is offered and records it — which is right when a board
/// was replaced or a printer factory reset, and is a deliberate press, not
/// something that happens by itself.
#[tauri::command]
fn host_forget_pin(host: State<PrinterHost>) -> Result<(), String> {
    let key = host
        .pin_key
        .lock()
        .map_err(|_| "the pin lock is poisoned".to_string())?
        .clone();
    if key.is_empty() {
        return Err("Connect a printer first.".to_string());
    }
    secrets::clear(&key)?;
    host.pin_stored
        .store(false, std::sync::atomic::Ordering::Relaxed);
    // The link on screen is still the refused one; closing it makes the next
    // Connect a fresh handshake rather than a reconnect into the old pin.
    let mut slot = host
        .link
        .lock()
        .map_err(|_| "the printer lock is poisoned".to_string())?;
    if let Some(link) = slot.take() {
        link.close();
    }
    Ok(())
}

/// The pin this printer is held to, or `None` when the next connection will be
/// its first sighting.
#[tauri::command]
fn host_pin(host: State<PrinterHost>) -> Result<Option<String>, String> {
    let key = host
        .pin_key
        .lock()
        .map_err(|_| "the pin lock is poisoned".to_string())?
        .clone();
    if key.is_empty() {
        return Ok(None);
    }
    secrets::get(&key)
}

/// Upload the plate that was just sliced and hand the machine the command to
/// print it. Two steps on purpose: the file lands first, so a failed upload
/// never leaves a print command pointing at a name that is not there.
/// Upload one file and tell the machine to print it. The send button and the
/// queue both come through here, so a queued job takes exactly the same path as
/// a pressed one — including the bed type and the sequence number.
// Does this G-code open with a machine's own start sequence? The rule lives in
// the protocol crate, next to the upload it protects: it used to read the whole
// file here (a sliced plate is tens of megabytes) and it matched the banner
// anywhere in it, so a Cura file with the string in a comment was accepted.
use shale_host::starts_with_machine_sequence;

/// The pin this printer is held to, if one has been recorded. Shared by the
/// status link and the upload so both are guarded by the same value.
fn stored_pin(serial: &str, host: &str) -> Option<shale_host::Pin> {
    let key = format!(
        "pin/{}",
        if serial.trim().is_empty() {
            host
        } else {
            serial
        }
    );
    let stored = secrets::get(&key).ok().flatten()?;
    shale_host::parse_hex(&stored)
}

async fn send_file(
    state: &State<'_, AppState>,
    host: &State<'_, PrinterHost>,
    path: PathBuf,
    bed_type: String,
) -> Result<serde_json::Value, String> {
    let (config, bambu) = {
        let mut inner = lock(state)?;
        let printer = inner.printer_id.clone();
        let bambu = match (catalog(&mut inner), printer.as_deref()) {
            (Ok(catalog), Some(id)) => catalog
                .printers()
                .iter()
                .any(|printer| printer.id == id && printer.source == "bambu"),
            _ => false,
        };
        (inner.host_config.clone(), bambu)
    };
    let config = config.ok_or_else(|| "Connect a printer first.".to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "shale.gcode".to_string());
    if bambu && !starts_with_machine_sequence(&path) {
        return Err(format!(
            "Refusing to send `{name}`: it does not open with the machine's own start \
             sequence, so the printer would not wipe its nozzle, level the bed or purge. \
             Slice again with a nozzle whose Bambu profile carries one — the A1 ships one \
             for 0.4 mm — or export the file and print it from Bambu Studio."
        ));
    }
    let upload = {
        let config = config.clone();
        let path = path.clone();
        let name = name.clone();
        let pin = stored_pin(&config.serial, &config.host);
        tauri::async_runtime::spawn_blocking(move || {
            shale_host::upload_with(&shale_host::Trust::new(pin), &config, &path, &name)
        })
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?
    };
    let remote = format!("/{}/{}", config.remote_dir.trim_matches('/'), name);
    let sequence = host
        .sequence
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    host.with(|link| {
        link.send(shale_host::gcode_file_command(sequence, &remote, &bed_type))
            .map_err(|err| err.to_string())
    })?;
    Ok(serde_json::json!({ "remote": remote, "bytes": upload }))
}

/// The send button: whatever the plate last sliced.
#[tauri::command]
async fn host_send(
    state: State<'_, AppState>,
    host: State<'_, PrinterHost>,
    bed_type: String,
) -> Result<serde_json::Value, String> {
    let path = {
        let inner = lock(&state)?;
        inner
            .gcode_path
            .clone()
            .ok_or_else(|| "Slice the plate first: there is no G-code to send.".to_string())?
    };
    send_file(&state, &host, path, bed_type).await
}

fn queue_views(inner: &Inner) -> Vec<QueueView> {
    inner
        .queue
        .iter()
        .map(|job| QueueView {
            id: job.id,
            name: job.name.clone(),
            bytes: job.bytes,
            running: inner.queue_running.as_deref() == Some(job.name.as_str()),
        })
        .collect()
}

/// Put the plate's slice in the queue. The file is copied: slicing the next
/// plate writes the same path, and a queue pointing at a file someone is about
/// to overwrite is a queue that prints the wrong job.
#[tauri::command]
fn queue_add(
    app: AppHandle,
    state: State<'_, AppState>,
    name: Option<String>,
) -> Result<Vec<QueueView>, String> {
    let mut inner = lock(&state)?;
    let source = inner
        .gcode_path
        .clone()
        .ok_or_else(|| "Slice the plate first: there is no G-code to queue.".to_string())?;
    let bytes = fs::metadata(&source)
        .map_err(|err| format!("Could not read the slice: {err}"))?
        .len();
    let id = inner.queue_next_id;
    inner.queue_next_id += 1;
    let dir = config_dir(&app)?.join("queue");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let target = dir.join(format!("{id}.gcode"));
    fs::copy(&source, &target).map_err(|err| format!("Could not copy the slice: {err}"))?;
    let name = name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("job {id}"));
    inner.queue.push(QueuedJob {
        id,
        name,
        path: target,
        bytes,
    });
    Ok(queue_views(&inner))
}

#[tauri::command]
fn queue_list(state: State<'_, AppState>) -> Result<Vec<QueueView>, String> {
    let inner = lock(&state)?;
    Ok(queue_views(&inner))
}

#[tauri::command]
fn queue_remove(state: State<'_, AppState>, id: u64) -> Result<Vec<QueueView>, String> {
    let mut inner = lock(&state)?;
    if let Some(index) = inner.queue.iter().position(|job| job.id == id) {
        let job = inner.queue.remove(index);
        let _ = fs::remove_file(&job.path);
        if inner.queue_running.as_deref() == Some(job.name.as_str()) {
            inner.queue_running = None;
        }
    }
    Ok(queue_views(&inner))
}

#[tauri::command]
fn queue_clear(state: State<'_, AppState>) -> Result<Vec<QueueView>, String> {
    let mut inner = lock(&state)?;
    inner.queue_run.stop("queue cleared");
    for job in inner.queue.drain(..) {
        let _ = fs::remove_file(&job.path);
    }
    inner.queue_running = None;
    Ok(Vec::new())
}

/// Send the head of the queue. Reached only from a pressed button: this is the
/// one path that starts a print the person did not press "print" for, and it is
/// why the queue's own button names every job before it starts.
#[tauri::command]
async fn queue_start_next(
    state: State<'_, AppState>,
    host: State<'_, PrinterHost>,
    bed_type: String,
) -> Result<serde_json::Value, String> {
    let job = {
        let mut inner = lock(&state)?;
        if inner.queue.is_empty() {
            return Err("The queue is empty.".to_string());
        }
        // The one gate that matters. A screen can ask to start a job as often
        // as it likes; only a press that armed the run, or a plate cleared by
        // hand, puts it in a state where a job may leave.
        if !inner.queue_run.may_start() {
            return Err(
                "The queue is waiting for a plate-cleared press: nothing goes out from this bed \
                 until someone says the last print has been taken off it."
                    .to_string(),
            );
        }
        inner.queue.remove(0)
    };
    match send_file(&state, &host, job.path.clone(), bed_type).await {
        Ok(sent) => {
            let mut inner = lock(&state)?;
            inner.queue_run.started();
            inner.queue_running = Some(job.name.clone());
            Ok(serde_json::json!({
                "name": job.name,
                "sent": sent,
                "remaining": inner.queue.len(),
                "queue": queue_views(&inner),
            }))
        }
        Err(err) => {
            // Nothing is lost: the job goes back to the head of the queue, and
            // the run stops. An error is not a reason to try the next one.
            let mut inner = lock(&state)?;
            inner.queue.insert(0, job);
            inner
                .queue_run
                .stop("queue stopped — the host refused the job");
            Err(err)
        }
    }
}

/// What the run is allowed to do next, flattened for the pane.
#[derive(Serialize)]
struct QueueRunView {
    /// idle | armed | printing | waiting_for_plate | stopped
    state: String,
    note: Option<String>,
}

fn run_state_name(state: shale_core::queue::RunState) -> &'static str {
    use shale_core::queue::RunState;
    match state {
        RunState::Idle => "idle",
        RunState::Armed => "armed",
        RunState::Printing => "printing",
        RunState::WaitingForPlate => "waiting_for_plate",
        RunState::Stopped => "stopped",
    }
}

fn run_view(update: shale_core::queue::Update) -> QueueRunView {
    QueueRunView {
        state: run_state_name(update.state).to_string(),
        note: update.note,
    }
}

/// The press that authorises the list. It authorises the first job; every job
/// after it needs `queue_plate_cleared`.
#[tauri::command]
fn queue_arm(state: State<'_, AppState>) -> Result<QueueRunView, String> {
    let mut inner = lock(&state)?;
    let update = inner.queue_run.arm();
    Ok(run_view(update))
}

/// A report from the machine: `idle`, `preparing`, `running`, `finish`,
/// `failed`. It can stop the run or park it, never start a job.
#[tauri::command]
fn queue_observe(state: State<'_, AppState>, machine: String) -> Result<QueueRunView, String> {
    // One mapping, in the crate that owns the state machine: a report's
    // `gcode_state` and this command's argument are the same words.
    let machine = shale_core::queue::Machine::from_report(&machine)
        .ok_or_else(|| format!("{machine} is not a machine state the queue knows"))?;
    let mut inner = lock(&state)?;
    let update = inner.queue_run.observe(machine);
    Ok(run_view(update))
}

/// The operator has taken the finished print off the bed. This is the only way
/// the run goes back to being allowed to send a job.
#[tauri::command]
fn queue_plate_cleared(state: State<'_, AppState>) -> Result<QueueRunView, String> {
    let mut inner = lock(&state)?;
    let update = inner.queue_run.plate_cleared();
    Ok(run_view(update))
}

/// Stop the run, with a reason the pane will show.
#[tauri::command]
fn queue_stop(state: State<'_, AppState>, why: Option<String>) -> Result<QueueRunView, String> {
    let mut inner = lock(&state)?;
    let why = why.unwrap_or_else(|| "queue stopped".to_string());
    let update = inner.queue_run.stop(&why);
    Ok(run_view(update))
}

/// Where the run stands, for a pane that has just opened.
#[tauri::command]
fn queue_run_state(state: State<'_, AppState>) -> Result<QueueRunView, String> {
    let inner = lock(&state)?;
    Ok(QueueRunView {
        state: run_state_name(inner.queue_run.state()).to_string(),
        note: None,
    })
}

/// The machine has stopped printing the queued job, so the pane stops calling
/// it the running one.
#[tauri::command]
fn queue_release(state: State<'_, AppState>) -> Result<Vec<QueueView>, String> {
    let mut inner = lock(&state)?;
    inner.queue_running = None;
    Ok(queue_views(&inner))
}

/// A job waiting for the machine.
#[derive(Clone, Debug)]
struct QueuedJob {
    id: u64,
    name: String,
    path: PathBuf,
    bytes: u64,
}

#[derive(Serialize)]
struct QueueView {
    id: u64,
    name: String,
    bytes: u64,
    /// True for the job the machine is printing right now.
    running: bool,
}

#[tauri::command]
fn host_control(host: State<PrinterHost>, command: String) -> Result<(), String> {
    let allowed = ["pause", "resume", "stop"];
    if !allowed.contains(&command.as_str()) {
        return Err(format!("{command} is not a print control"));
    }
    let sequence = host
        .sequence
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    host.with(|link| {
        link.send(shale_host::simple_command(sequence, &command))
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
fn existing_slice(state: State<AppState>) -> Result<Option<SliceSummary>, String> {
    let inner = lock(&state)?;
    let Some(toolpath) = &inner.toolpath else {
        return Ok(None);
    };
    let Some(path) = &inner.gcode_path else {
        return Ok(None);
    };
    let facts = shale_core::gcode::header_facts(&read_head(path, 64 * 1024));
    Ok(Some(SliceSummary {
        layer_count: toolpath.layers.len(),
        layers: toolpath
            .layers
            .iter()
            .map(|layer| LayerMark {
                index: layer.index,
                z: layer.z,
            })
            .collect(),
        print_time_seconds: facts.seconds,
        filament_metres: facts.filament_metres,
        warning_count: 0,
        gcode_path: path.display().to_string(),
        model_ids: inner
            .models
            .iter()
            .map(|model| u64::from(model.id))
            .collect(),
        machine_gcode: starts_with_machine_sequence(path),
    }))
}

/// Write the plate out as geometry rather than G-code. STL merges the visible
/// parts into one blob, the way every slicer exports a plate; 3MF keeps them as
/// separate named objects. Both bake the model transforms, so the file holds the
/// plate as it looks.
/// Replace one part's overrides. The engine receives them beside that part's
/// mesh, so a single plate can carry different walls, infill or speed per part.
#[tauri::command]
fn set_model_settings(
    state: State<AppState>,
    id: u32,
    settings: std::collections::BTreeMap<String, String>,
) -> Result<ModelView, String> {
    let mut inner = lock(&state)?;
    let index = inner
        .models
        .iter()
        .position(|model| model.id == id)
        .ok_or_else(|| format!("no part {id}"))?;
    inner.models[index].settings = settings
        .into_iter()
        .filter(|(key, value)| !key.trim().is_empty() && !value.trim().is_empty())
        .collect();
    // The plate changed, so any existing G-code is stale.
    inner.toolpath = None;
    inner.gcode_path = None;
    let model = &inner.models[index];
    Ok(view_of(
        model.id,
        &model.name,
        &model.transform,
        &model.report,
        &model.settings,
        model.slot.as_ref(),
    ))
}

#[tauri::command]
fn set_model_slot(
    state: State<AppState>,
    id: u32,
    slot: Option<usize>,
) -> Result<ModelView, String> {
    let mut inner = lock(&state)?;
    let index = inner
        .models
        .iter()
        .position(|model| model.id == id)
        .ok_or_else(|| format!("no part {id}"))?;
    // A Bambu AMS has four spools, so a part can point at T0..T3 and no further.
    if let Some(slot) = slot {
        if slot > 3 {
            return Err(format!("slot {slot} is past the fourth spool"));
        }
    }
    inner.models[index].slot = slot;
    // Which spool a part uses changes the toolpath, so the old one is stale.
    inner.toolpath = None;
    inner.gcode_path = None;
    let model = &inner.models[index];
    Ok(view_of(
        model.id,
        &model.name,
        &model.transform,
        &model.report,
        &model.settings,
        model.slot.as_ref(),
    ))
}

#[tauri::command]
fn export_plate(
    state: State<AppState>,
    destination: String,
    format: String,
) -> Result<usize, String> {
    let inner = lock(&state)?;
    let visible: Vec<(&str, TriangleMesh)> = inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| {
            (
                model.name.as_str(),
                shale_core::mesh::bake(&model.mesh, &model.transform),
            )
        })
        .collect();
    if visible.is_empty() {
        return Err("Nothing on the plate to export".into());
    }
    let triangles: usize = visible.iter().map(|(_, mesh)| mesh.triangles.len()).sum();
    if let Some(parent) = PathBuf::from(&destination).parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    match format.as_str() {
        "stl" => {
            let meshes: Vec<TriangleMesh> = visible.into_iter().map(|(_, mesh)| mesh).collect();
            let merged = shale_core::exchange::merge(&meshes);
            fs::write(&destination, shale_core::exchange::to_stl(&merged))
                .map_err(|err| err.to_string())?;
        }
        "3mf" => {
            let parts: Vec<(&str, &TriangleMesh)> =
                visible.iter().map(|(name, mesh)| (*name, mesh)).collect();
            let bytes = shale_core::exchange::to_3mf(&parts)?;
            fs::write(&destination, bytes).map_err(|err| err.to_string())?;
        }
        other => return Err(format!("Unknown export format {other}")),
    }
    Ok(triangles)
}

#[tauri::command]
fn export_gcode(state: State<AppState>, destination: String) -> Result<String, String> {
    let inner = lock(&state)?;
    let Some(source) = &inner.gcode_path else {
        return Err("Slice a model before exporting G-code".into());
    };
    if let Some(parent) = PathBuf::from(&destination).parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::copy(source, &destination).map_err(|err| err.to_string())?;
    Ok(destination)
}

fn bed_from(resolved: &shale_core::ResolvedSettings) -> Bed {
    let summary = &resolved.summary;
    Bed {
        width: summary.machine_width,
        depth: summary.machine_depth,
        height: summary.machine_height,
        center_is_zero: summary.machine_center_is_zero,
        layer_height: summary.layer_height,
        infill_sparse_density: summary.infill_sparse_density,
        support_enable: summary.support_enable,
        nozzle_temperature: summary.nozzle_temperature,
        bed_temperature: summary.bed_temperature,
        nozzle_size: summary.nozzle_size,
        line_width: summary.line_width,
        adhesion_type: summary.adhesion_type.clone(),
        speed_print: summary.speed_print,
        material_diameter: summary.material_diameter,
        warning_count: resolved.warning_count,
    }
}

fn view_of(
    id: u32,
    name: &str,
    transform: &Transform,
    report: &ImportReport,
    settings: &std::collections::BTreeMap<String, String>,
    slot: Option<&usize>,
) -> ModelView {
    ModelView {
        id,
        name: name.to_string(),
        positions: Vec::new(),
        transform: transform.clone(),
        triangles: report.triangles,
        max_deviation_mm: report.max_deviation_mm,
        max_turning_deg: report.max_turning_deg,
        import_ms: report.milliseconds,
        source: report.source.clone(),
        settings: settings.clone(),
        slot: slot.copied(),
    }
}

fn preload_demo(
    catalog: &Catalog,
    engine: &Engine,
) -> (
    Vec<LoadedModel>,
    Vec<(u32, CustomInstruction)>,
    Option<Toolpath>,
    Option<PathBuf>,
) {
    let Some(path) = std::env::var_os("SHALE_DEMO_MODEL") else {
        return (Vec::new(), Vec::new(), None, None);
    };
    // The model is always preloaded, so the window opens on a plate that
    // already holds the demo part instead of racing an import it cannot see.
    // The slice and the support plan are still optional.
    let Ok(imported) = load_mesh_with(
        std::path::Path::new(&path),
        &TessellationTolerance::standard(),
    ) else {
        return (Vec::new(), Vec::new(), None, None);
    };
    let mesh = imported.mesh;
    let report = imported.report;
    let printer_id = if catalog.printer("creality_ender3").is_some() {
        "creality_ender3".to_string()
    } else if let Some(printer) = catalog.printers().first() {
        printer.id.clone()
    } else {
        return (Vec::new(), Vec::new(), None, None);
    };
    let resolved = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: printer_id.clone(),
            variant_name: Some("0.4mm Nozzle".into()),
            quality_type: Some("draft".into()),
            material_id: Some("generic_pla".into()),
            intent_category: None,
            settings: UserSettings::default(),
        })
        .ok();
    let transform = if let Some(resolved) = &resolved {
        place_on_bed(
            &mesh,
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_center_is_zero,
        )
    } else {
        let mut transform = Transform::default();
        drop_to_bed(&mesh, &mut transform);
        transform
    };
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model")
        .to_string();
    let mut toolpath = None;
    let mut gcode_path = None;
    let mut instructions = Vec::new();
    // Two parts on two spools, with supports and a prime tower: the shape of a
    // real multi-colour plate, which is what the "multi" clip has to show.
    let multi = std::env::var("SHALE_DEMO_SUPPORTS").ok().as_deref() == Some("1");
    let mut placed = vec![PlacedModel {
        name: name.clone(),
        mesh: mesh.clone(),
        transform: transform.clone(),
        settings: Vec::new(),
        slot: None,
    }];
    if multi {
        let mut second = transform.clone();
        second.translation[0] += 45.0;
        placed.push(PlacedModel {
            name: format!("{name} spool 2"),
            mesh: mesh.clone(),
            transform: second,
            settings: Vec::new(),
            slot: Some(1),
        });
    }
    let materials: Vec<Option<String>> = if multi {
        vec![Some("generic_pla".into()), Some("generic_petg".into())]
    } else {
        vec![Some("generic_pla".into())]
    };
    if std::env::var("SHALE_DEMO_SLICE").ok().as_deref() == Some("1") {
        let pause = CustomInstruction::Pause {
            layer: Some(40),
            height_mm: None,
            filament_change: true,
            message: Some("swap filament".into()),
        };
        instructions.push((1, pause.clone()));
        let output = std::env::temp_dir().join("shale").join("demo.gcode");
        if let Ok(outcome) = slice(
            catalog,
            engine,
            &printer_id,
            Some("0.4mm Nozzle".into()),
            Some("draft".into()),
            &materials,
            UserSettings {
                layer_height: Some(0.2),
                infill_sparse_density: Some(20.0),
                support_enable: Some(multi),
                material_print_temperature: Some(200.0),
                material_bed_temperature: Some(60.0),
                extra: if multi {
                    vec![("prime_tower_enable".to_string(), "true".to_string())]
                } else {
                    Vec::new()
                },
            },
            &placed,
            &[pause],
            None,
            &[],
            output,
            &mut |_| {},
        ) {
            gcode_path = Some(outcome.gcode_path);
            toolpath = Some(outcome.toolpath);
        }
    }
    let source_bytes = fs::read(std::path::Path::new(&path)).unwrap_or_default();
    let mut loaded = vec![LoadedModel {
        id: 1,
        name,
        mesh,
        transform,
        report,
        source_bytes: source_bytes.clone(),
        visible: true,
        settings: std::collections::BTreeMap::new(),
        slot: None,
    }];
    if multi {
        if let Some(second) = placed.get(1) {
            loaded.push(LoadedModel {
                id: 2,
                name: second.name.clone(),
                mesh: second.mesh.clone(),
                transform: second.transform.clone(),
                report: loaded[0].report.clone(),
                source_bytes,
                visible: true,
                settings: std::collections::BTreeMap::new(),
                slot: Some(1),
            });
        }
    }
    (loaded, instructions, toolpath, gcode_path)
}

#[derive(Deserialize)]
struct SaveRequest {
    path: String,
    name: String,
    printer_id: String,
    printer_label: String,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    #[serde(default)]
    ams: Vec<String>,
    settings: UserSettings,
    chord_mm: f64,
    turn_deg: f64,
    thumbnail: Vec<u8>,
    /// A plate parked behind another tab is written so it is not lost, but it
    /// is not something the person asked for: it has no business in the list of
    /// projects they opened, and it must not push a real one off that list.
    #[serde(default)]
    quiet: bool,
}

#[derive(Serialize)]
struct OpenedJob {
    path: String,
    name: String,
    printer_id: String,
    printer_label: String,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    ams: Vec<String>,
    settings: UserSettings,
    chord_mm: f64,
    turn_deg: f64,
    models: Vec<ModelView>,
    instructions: Vec<StoredInstruction>,
    support: Option<SupportView>,
    warnings: Vec<String>,
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|err| err.to_string())?;
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

fn remember(
    app: &AppHandle,
    path: &str,
    name: &str,
    printer: &str,
) -> Result<Vec<RecentEntry>, String> {
    let dir = config_dir(app)?;
    remember_recent(
        &dir,
        &RecentEntry {
            path: path.to_string(),
            name: name.to_string(),
            saved_at_ms: now_ms(),
            printer: printer.to_string(),
            missing: false,
        },
    )
    .map_err(|err| err.to_string())
}

/// The cost fields, read from the app's config dir.
///
/// A missing or unreadable file is the defaults, not an error: a fresh install
/// has no file, and a cost estimate is not worth refusing a slice over.
/// Where named presets live: `presets/` beside the other app files.
fn preset_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join("presets"))
}

#[tauri::command]
fn preset_list(app: AppHandle) -> Result<Vec<shale_core::preset::Preset>, String> {
    shale_core::preset::list(&preset_dir(&app)?).map_err(|err| err.to_string())
}

#[tauri::command]
fn preset_save(
    app: AppHandle,
    name: String,
    values: std::collections::BTreeMap<String, String>,
    replace: bool,
) -> Result<shale_core::preset::Preset, String> {
    let preset = shale_core::preset::Preset::new(&name, values).map_err(|err| err.to_string())?;
    let dir = preset_dir(&app)?;
    let result = if replace {
        shale_core::preset::overwrite(&dir, &preset)
    } else {
        shale_core::preset::save(&dir, &preset)
    };
    result.map_err(|err| err.to_string())?;
    Ok(preset)
}

#[tauri::command]
fn preset_delete(app: AppHandle, name: String) -> Result<(), String> {
    shale_core::preset::delete(&preset_dir(&app)?, &name).map_err(|err| err.to_string())
}

#[tauri::command]
fn preset_rename(
    app: AppHandle,
    from: String,
    to: String,
) -> Result<shale_core::preset::Preset, String> {
    shale_core::preset::rename(&preset_dir(&app)?, &from, &to).map_err(|err| err.to_string())
}

#[tauri::command]
fn preset_export(app: AppHandle, name: String, destination: String) -> Result<(), String> {
    shale_core::preset::export(
        &preset_dir(&app)?,
        &name,
        std::path::Path::new(&destination),
    )
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn preset_import(app: AppHandle, source: String) -> Result<shale_core::preset::Preset, String> {
    shale_core::preset::import(&preset_dir(&app)?, std::path::Path::new(&source))
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn cost_settings(app: AppHandle) -> Result<shale_core::cost::CostSettings, String> {
    let dir = config_dir(&app)?;
    let path = dir.join("cost.json");
    match fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
        Err(_) => Ok(shale_core::cost::CostSettings::default()),
    }
}

#[tauri::command]
fn set_cost_settings(
    app: AppHandle,
    settings: shale_core::cost::CostSettings,
) -> Result<shale_core::cost::CostSettings, String> {
    let mut settings = settings;
    settings.price_per_kg = settings.price_per_kg.clamp(0.0, 10_000.0);
    settings.watts = settings.watts.clamp(0.0, 20_000.0);
    settings.electricity_per_kwh = settings.electricity_per_kwh.clamp(0.0, 100.0);
    if settings.currency.trim().is_empty() {
        settings.currency = shale_core::cost::DEFAULT_CURRENCY.to_string();
    }
    settings.currency = settings.currency.trim().chars().take(6).collect();
    let dir = config_dir(&app)?;
    let path = dir.join("cost.json");
    let text = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    fs::write(&path, text).map_err(|err| err.to_string())?;
    Ok(settings)
}

/// What the slice on screen costs, in the currency the user chose.
#[tauri::command]
fn cost_estimate(
    app: AppHandle,
    metres: f64,
    seconds: Option<f64>,
    diameter: f64,
    density: f64,
) -> Result<shale_core::cost::CostEstimate, String> {
    let settings = cost_settings(app)?;
    Ok(shale_core::cost::estimate(
        &[(0, metres.max(0.0))],
        if diameter > 0.0 { diameter } else { 1.75 },
        if density > 0.0 { density } else { 1.24 },
        seconds,
        &settings,
    ))
}

#[tauri::command]
fn list_recents(app: AppHandle) -> Result<Vec<RecentEntry>, String> {
    let dir = config_dir(&app)?;
    load_recents(&dir).map_err(|err| err.to_string())
}

#[tauri::command]
fn forget_recent_project(app: AppHandle, path: String) -> Result<Vec<RecentEntry>, String> {
    let dir = config_dir(&app)?;
    forget_recent(&dir, &path).map_err(|err| err.to_string())
}

/// A plate as the window sees it: its name and how many parts are on it.
#[derive(Clone, Debug, serde::Serialize)]
struct PlateView {
    index: usize,
    name: String,
    models: usize,
    active: bool,
}

#[derive(serde::Serialize)]
struct PlateSwitch {
    plates: Vec<PlateView>,
    models: Vec<ModelView>,
}

impl Inner {
    fn plate_names(&self) -> Vec<String> {
        self.plates
            .iter()
            .enumerate()
            .map(|(index, plate)| {
                if plate.name.is_empty() {
                    format!("plate {}", index + 1)
                } else {
                    plate.name.clone()
                }
            })
            .collect()
    }

    fn plate_views(&self) -> Vec<PlateView> {
        self.plates
            .iter()
            .enumerate()
            .map(|(index, plate)| PlateView {
                index,
                name: if plate.name.is_empty() {
                    format!("plate {}", index + 1)
                } else {
                    plate.name.clone()
                },
                models: if index == self.active_plate {
                    self.models.iter().filter(|model| model.visible).count()
                } else {
                    plate.models.iter().filter(|model| model.visible).count()
                },
                active: index == self.active_plate,
            })
            .collect()
    }

    /// Move the plate in front into its stash, so the next one can take its
    /// place. The support plan travels with the plate it was proposed on.
    fn park_plate(&mut self) {
        if let Some(slot) = self.plates.get_mut(self.active_plate) {
            slot.models = std::mem::take(&mut self.models);
            slot.support = self.support.take();
            slot.support_model_id = self.support_model_id.take();
        }
    }

    /// Bring a stashed plate to the front. Whatever was in front is parked
    /// first, so this is always a swap.
    fn resume_plate(&mut self, index: usize) {
        if index == self.active_plate || index >= self.plates.len() {
            return;
        }
        self.park_plate();
        self.active_plate = index;
        let slot = &mut self.plates[index];
        self.models = std::mem::take(&mut slot.models);
        self.support = slot.support.take();
        self.support_model_id = slot.support_model_id.take();
        // A slice belongs to the plate that was sliced.
        self.toolpath = None;
        self.gcode_path = None;
    }
}

#[tauri::command]
fn list_plates(state: State<AppState>) -> Result<Vec<PlateView>, String> {
    let inner = lock(&state)?;
    Ok(inner.plate_views())
}

/// Add an empty plate and turn to it, the way Bambu's plus button does.
#[tauri::command]
fn add_plate(state: State<AppState>) -> Result<PlateSwitch, String> {
    let mut inner = lock(&state)?;
    inner.park_plate();
    let name = format!("plate {}", inner.plates.len() + 1);
    inner.plates.push(PlateStash {
        name,
        ..PlateStash::default()
    });
    let index = inner.plates.len() - 1;
    inner.active_plate = index;
    inner.models.clear();
    inner.support = None;
    inner.support_model_id = None;
    inner.toolpath = None;
    inner.gcode_path = None;
    let plates = inner.plate_views();
    Ok(PlateSwitch {
        plates,
        models: Vec::new(),
    })
}

#[tauri::command]
fn switch_plate(state: State<AppState>, index: usize) -> Result<PlateSwitch, String> {
    let mut inner = lock(&state)?;
    if index >= inner.plates.len() {
        return Err(format!("There is no plate {}.", index + 1));
    }
    inner.resume_plate(index);
    let views: Vec<ModelView> = inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| {
            view_of(
                model.id,
                &model.name,
                &model.transform,
                &model.report,
                &model.settings,
                model.slot.as_ref(),
            )
        })
        .collect();
    let plates = inner.plate_views();
    Ok(PlateSwitch {
        plates,
        models: views,
    })
}

/// A project with no plates cannot be sliced, so the last one stays.
#[tauri::command]
fn delete_plate(state: State<AppState>, index: usize) -> Result<Vec<PlateView>, String> {
    let mut inner = lock(&state)?;
    if inner.plates.len() <= 1 {
        return Err("A project needs at least one plate.".to_string());
    }
    if index >= inner.plates.len() {
        return Err(format!("There is no plate {}.", index + 1));
    }
    if index == inner.active_plate {
        // Step off the plate being removed before it goes.
        let other = if index == 0 { 1 } else { 0 };
        inner.resume_plate(other);
    }
    inner.plates.remove(index);
    // The stash array shifted, so the index of the plate in front moved too.
    let active = if index < inner.active_plate {
        inner.active_plate - 1
    } else {
        inner.active_plate
    };
    inner.active_plate = active.min(inner.plates.len() - 1);
    Ok(inner.plate_views())
}

#[tauri::command]
fn rename_plate(
    state: State<AppState>,
    index: usize,
    name: String,
) -> Result<Vec<PlateView>, String> {
    let mut inner = lock(&state)?;
    let plate = inner
        .plates
        .get_mut(index)
        .ok_or_else(|| format!("There is no plate {}.", index + 1))?;
    let trimmed = name.trim();
    plate.name = if trimmed.is_empty() {
        format!("plate {}", index + 1)
    } else {
        trimmed.chars().take(48).collect()
    };
    Ok(inner.plate_views())
}

#[tauri::command]
fn clear_job(state: State<AppState>) -> Result<(), String> {
    let mut inner = lock(&state)?;
    inner.models.clear();
    inner.plates = vec![PlateStash {
        name: "plate 1".to_string(),
        ..PlateStash::default()
    }];
    inner.active_plate = 0;
    inner.next_id = 1;
    inner.instructions.clear();
    inner.next_instruction = 1;
    inner.support = None;
    inner.support_model_id = None;
    inner.toolpath = None;
    inner.gcode_path = None;
    Ok(())
}

/// A path under the app's scratch directory for a project that has never been
/// saved. The tab bar writes a tab here before switching away from it, so a
/// plate you have not named yet survives a click on another tab.
#[tauri::command]
fn scratch_path(name: String) -> String {
    let mut safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    while safe.contains("--") {
        safe = safe.replace("--", "-");
    }
    let safe = safe.trim_matches('-').to_string();
    let stem = if safe.is_empty() {
        "untitled".to_string()
    } else {
        safe
    };
    let dir = std::env::temp_dir().join("shale").join("tabs");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{stem}.shale"))
        .to_string_lossy()
        .into_owned()
}

#[tauri::command]
fn save_project(
    app: AppHandle,
    state: State<AppState>,
    project: SaveRequest,
) -> Result<Vec<RecentEntry>, String> {
    let inner = lock(&state)?;
    // `support_model` indexes the flat list of every plate's parts, so a plan
    // proposed on plate 2 has to be shifted past the parts of plate 1.
    let before: usize = inner
        .plates
        .iter()
        .take(inner.active_plate)
        .map(|plate| plate.models.iter().filter(|model| model.visible).count())
        .sum();
    let support_model = inner
        .support_model_id
        .and_then(|id| inner.models.iter().position(|model| model.id == id))
        .map(|index| index + before);
    let mut support = inner.support.clone();
    if support_model.is_none() {
        support = None;
    }
    let mut models: Vec<ProjectModel> = Vec::new();
    for (index, plate) in inner.plates.iter().enumerate() {
        let list = if index == inner.active_plate {
            &inner.models
        } else {
            &plate.models
        };
        for model in list.iter().filter(|model| model.visible) {
            if model.source_bytes.is_empty() {
                return Err(format!(
                    "Cannot save {}: the original file is missing.",
                    model.name
                ));
            }
            models.push(ProjectModel {
                name: model.name.clone(),
                bytes: model.source_bytes.clone(),
                transform: model.transform.clone(),
                settings: model.settings.clone(),
                slot: model.slot,
                plate: index,
                painted: inner
                    .painted
                    .get(&model.id)
                    .map(|paint| paint.enforce.clone())
                    .unwrap_or_default(),
                blocked: inner
                    .painted
                    .get(&model.id)
                    .map(|paint| paint.block.clone())
                    .unwrap_or_default(),
            });
        }
    }
    let data = ProjectData {
        name: project.name.clone(),
        printer_id: project.printer_id,
        printer_label: project.printer_label.clone(),
        variant_name: project.variant_name,
        quality_type: project.quality_type,
        material_id: project.material_id,
        ams: project.ams,
        settings: project.settings,
        chord_mm: project.chord_mm,
        turn_deg: project.turn_deg,
        models,
        plate_names: inner.plate_names(),
        instructions: inner
            .instructions
            .iter()
            .map(|(_, instruction)| instruction.clone())
            .collect(),
        support,
        support_model,
        thumbnail: project.thumbnail,
    };
    drop(inner);
    let path = PathBuf::from(&project.path);
    shale_core::project::write_project(&path, &data).map_err(|err| err.to_string())?;
    if project.quiet {
        // Written, not remembered: a parked plate is still a draft.
        let dir = config_dir(&app)?;
        return load_recents(&dir).map_err(|err| err.to_string());
    }
    remember(&app, &project.path, &project.name, &project.printer_label)
}

#[tauri::command]
async fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<OpenedJob, String> {
    let file = PathBuf::from(&path);
    let loaded = tauri::async_runtime::spawn_blocking(move || {
        let project = read_project(&file).map_err(|err| err.to_string())?;
        let tolerance = TessellationTolerance::new(
            if project.data.chord_mm > 0.0 {
                project.data.chord_mm
            } else {
                0.1
            },
            if project.data.turn_deg > 0.0 {
                project.data.turn_deg
            } else {
                15.0
            },
        );
        let mut built = Vec::new();
        for model in &project.data.models {
            let imported = load_named_bytes(
                &model.name,
                &model.bytes,
                &tolerance,
                &mut |_, _| {},
                &AtomicBool::new(false),
            )
            .map_err(|err| format!("Could not read {}: {err}", model.name))?;
            built.push((
                model.name.clone(),
                model.bytes.clone(),
                model.transform.clone(),
                imported,
                model.settings.clone(),
                model.slot,
                model.plate,
                model.painted.clone(),
                model.blocked.clone(),
            ));
        }
        Ok::<_, String>((project, built))
    })
    .await
    .map_err(|err| format!("open task failed: {err}"))??;
    let (project, built) = loaded;
    let mut inner = lock(&state)?;
    inner.models.clear();
    inner.painted.clear();
    inner.instructions.clear();
    inner.support = None;
    inner.support_model_id = None;
    inner.toolpath = None;
    inner.gcode_path = None;
    inner.next_id = 1;
    inner.next_instruction = 1;
    // One plate per name, and at least enough plates to hold every model, so a
    // file written before plates existed still opens with one.
    let names = project.data.plate_names.clone();
    let plate_count = names
        .len()
        .max(built.iter().map(|entry| entry.6 + 1).max().unwrap_or(1))
        .max(1);
    inner.plates = (0..plate_count)
        .map(|index| PlateStash {
            name: names
                .get(index)
                .cloned()
                .unwrap_or_else(|| format!("plate {}", index + 1)),
            ..PlateStash::default()
        })
        .collect();
    inner.active_plate = 0;
    let mut views = Vec::new();
    let mut ids: Vec<u32> = Vec::new();
    for (name, bytes, transform, imported, settings, slot, plate, painted, blocked) in built {
        let id = inner.next_id;
        inner.next_id += 1;
        if !painted.is_empty() || !blocked.is_empty() {
            inner.painted.insert(
                id,
                Paint {
                    enforce: painted,
                    block: blocked,
                },
            );
        }
        let view = view_of(
            id,
            &name,
            &transform,
            &imported.report,
            &settings,
            slot.as_ref(),
        );
        ids.push(id);
        let model = LoadedModel {
            id,
            name,
            mesh: imported.mesh,
            transform,
            report: imported.report,
            source_bytes: bytes,
            visible: true,
            settings,
            slot,
        };
        if plate == 0 {
            inner.models.push(model);
            views.push(view);
        } else if let Some(stash) = inner.plates.get_mut(plate) {
            stash.models.push(model);
        }
    }
    for instruction in project.data.instructions.clone() {
        let id = inner.next_instruction;
        inner.next_instruction += 1;
        inner.instructions.push((id, instruction));
    }
    if let Some(index) = project.data.support_model {
        if let Some(&id) = ids.get(index) {
            let plate = project
                .data
                .models
                .get(index)
                .map(|model| model.plate)
                .unwrap_or(0);
            if plate == 0 {
                inner.support_model_id = Some(id);
                inner.support = project.data.support.clone();
            } else if let Some(stash) = inner.plates.get_mut(plate) {
                stash.support_model_id = Some(id);
                stash.support = project.data.support.clone();
            }
        }
    }
    let instructions = stored_instructions(&inner);
    let support = inner.support.as_ref().map(support_view);
    drop(inner);
    let printer = if project.data.printer_label.is_empty() {
        project.data.printer_id.clone()
    } else {
        project.data.printer_label.clone()
    };
    let recents_error = remember(&app, &path, &project.data.name, &printer).err();
    let mut warnings = project.warnings;
    if let Some(err) = recents_error {
        warnings.push(format!("Could not update the recent list: {err}"));
    }
    Ok(OpenedJob {
        path,
        name: project.data.name,
        printer_id: project.data.printer_id,
        printer_label: project.data.printer_label,
        variant_name: project.data.variant_name,
        quality_type: project.data.quality_type,
        material_id: project.data.material_id,
        ams: project.data.ams,
        settings: project.data.settings,
        chord_mm: project.data.chord_mm,
        turn_deg: project.data.turn_deg,
        models: views,
        instructions,
        support,
        warnings,
    })
}

/// Print how long each startup step took. The window is created only after
/// these finish, so a slow one is the difference between an app that opens and
/// an app that looks dead; `SHALE_TRACE=1` turns it on.
fn trace(step: &str, started: &std::time::Instant) {
    if std::env::var_os("SHALE_TRACE").is_some() {
        eprintln!("[shale] {step}: {:?}", started.elapsed());
    }
}

/// Store the printer's LAN access code in the Windows Credential Manager.
///
/// The value is a password on the machine's MQTT connection; it must not be
/// written to the WebView profile, a log line, or a saved project.
#[tauri::command]
fn secret_set(key: String, value: String) -> Result<(), String> {
    secrets::set(&key, &value)
}

/// Read a stored secret, or `None` when nothing is stored under that key.
#[tauri::command]
fn secret_get(key: String) -> Result<Option<String>, String> {
    secrets::get(&key)
}

/// Forget a stored secret. Clearing an absent key succeeds.
#[tauri::command]
fn secret_clear(key: String) -> Result<(), String> {
    secrets::clear(&key)
}

/// The file a launch was asked to open, if any.
///
/// Windows hands a double-clicked file over as a bare argument, so the first
/// argument with a model or project extension is the request. Flags are
/// skipped, and an extension Shale cannot open is ignored rather than shown as
/// an error: Explorer can be told to "open with" anything.
fn open_argument() -> Option<String> {
    for arg in std::env::args_os().skip(1) {
        let text = arg.to_string_lossy().to_string();
        if text.starts_with('-') {
            continue;
        }
        let lower = text.to_lowercase();
        if [".shale", ".3mf", ".stl", ".step", ".stp"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        {
            return Some(text);
        }
    }
    None
}

/// Where a second launch leaves the file it was asked to open.
///
/// The running Shale owns the window, so the second process cannot open a tab;
/// it writes the path here, brings the window up and exits. The running app
/// polls this file, which is crude and has no failure mode that loses the
/// request quietly.
fn pending_open_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join("dev.shale.desktop")
            .join("pending-open.txt"),
    )
}

static STARTUP_OPEN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// The file this launch was asked to open. Answered once per process.
#[tauri::command]
fn startup_open() -> Option<String> {
    STARTUP_OPEN.get().cloned().flatten()
}

/// A file another launch handed over, consumed by reading it.
#[tauri::command]
fn take_pending_open() -> Option<String> {
    let path = pending_open_path()?;
    let text = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// The scenario named by `--demo <scenario>`, if any.
fn demo_flag() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--demo" {
            return args.next();
        }
        if let Some(value) = arg.strip_prefix("--demo=") {
            return Some(value.to_string());
        }
    }
    None
}

/// A cube that travels with the binary, so `--demo` works from an install.
///
/// It is written to the temp folder rather than opened from the repository:
/// the installed app has no repository, and a demo that only runs from a
/// checkout is a demo that breaks the first time someone else runs it.
fn demo_model() -> std::path::PathBuf {
    let path = std::env::temp_dir().join("shale-demo-cube.stl");
    let _ = std::fs::write(&path, include_bytes!("../../../../examples/cube.stl"));
    path
}

/// Turn `--demo <scenario>` into the environment the rest of the app reads.
///
/// The scenarios are states, not animations: each one is the app already in the
/// position a video wants to start from, so the recording is deterministic and
/// does not depend on a robot finding a button.
fn apply_demo_flag() {
    let Some(scenario) = demo_flag() else {
        return;
    };
    // Safety: called before any thread is spawned, at the top of run().
    unsafe {
        std::env::set_var("SHALE_DEMO", &scenario);
        if scenario != "start" {
            std::env::set_var("SHALE_DEMO_MODEL", demo_model());
        }
        if matches!(
            scenario.as_str(),
            "preview" | "multi" | "queue" | "cost" | "presets"
        ) {
            std::env::set_var("SHALE_DEMO_SLICE", "1");
        }
        if scenario == "multi" {
            std::env::set_var("SHALE_DEMO_SUPPORTS", "1");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    apply_demo_flag();
    // Before anything opens: WebView2 will not let two processes share the
    // window's profile, and its refusal is a bare HRESULT that unwinds setup.
    let requested = open_argument();
    let _ = STARTUP_OPEN.set(requested.clone());
    if !instance::claim("Local\\dev.shale.desktop") {
        // A second launch means "show me Shale": bring the window up rather than
        // telling the user off. If it named a file, leave the path where the
        // running window will find it before raising the window.
        if let Some(path) = &requested {
            if let Some(pending) = pending_open_path() {
                if let Some(parent) = pending.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(&pending, path);
            }
        }
        if !instance::focus_existing("Shale") {
            instance::say_already_running();
        }
        return;
    }
    let started = std::time::Instant::now();
    let engine = Engine::from_env();
    // The profile library is read on a worker thread: two and a half seconds of
    // blank window on a machine with Cura and Bambu Studio installed is not a
    // good first impression, and a slow or broken library should never stop the
    // window from opening. Commands wait for it through `catalog()`.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Two steps, timed separately under `SHALE_TRACE`: finding Cura is a
        // directory walk and reading it is eight thousand JSON files, and when
        // the first window takes a minute it matters which one it is.
        let traced = std::env::var_os("SHALE_TRACE").is_some();
        let step = std::time::Instant::now();
        let discovered = ResourcePaths::discover();
        if traced {
            eprintln!("[shale] resources discovered: {:?}", step.elapsed());
        }
        let step = std::time::Instant::now();
        let read = match discovered {
            Ok(paths) => match Catalog::open(paths) {
                Ok(catalog) => Ok(catalog),
                Err(err) => Err(err.to_string()),
            },
            Err(err) => Err(err.to_string()),
        };
        if traced {
            eprintln!("[shale] catalogue read: {:?}", step.elapsed());
        }
        let _ = tx.send(read);
    });
    // The demo state is dev-only and slices at startup, so it waits here.
    let demo_wanted = std::env::var_os("SHALE_DEMO_MODEL").is_some();
    let (catalog, catalog_error, catalog_rx, demo_catalog) = if demo_wanted {
        match rx.recv() {
            Ok(Ok(catalog)) => (Some(catalog), None, None, true),
            Ok(Err(err)) => (None, Some(err), None, false),
            Err(_) => (
                None,
                Some("the profile library reader stopped".into()),
                None,
                false,
            ),
        }
    } else {
        (None, None, Some(rx), false)
    };
    let catalog_loading = catalog.is_none() && catalog_error.is_none();
    trace("catalog", &started);
    let engine_message = match engine.cli.probe() {
        Ok(()) => format!("CuraEngine answered at {}", engine.cli.program.display()),
        Err(err) => err,
    };
    trace("engine", &started);
    let import_cancel = Arc::new(AtomicBool::new(false));
    let (models, instructions, toolpath, gcode_path) = if demo_catalog {
        match &catalog {
            Some(catalog) => preload_demo(catalog, &engine),
            None => (Vec::new(), Vec::new(), None, None),
        }
    } else {
        (Vec::new(), Vec::new(), None, None)
    };
    trace("demo", &started);
    let next_id = models.len() as u32 + 1;
    let next_instruction = instructions.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
    let support = if std::env::var("SHALE_DEMO_SUPPORTS").ok().as_deref() == Some("1") {
        models.first().map(|model| {
            let baked = shale_core::mesh::bake(&model.mesh, &model.transform);
            propose(&baked, &SupportParams::default())
        })
    } else {
        None
    };
    let support_model_id = if support.is_some() { Some(1) } else { None };
    // A queue clip needs a queue. These jobs point at the demo slice: nothing
    // is printed, and the pane shows names, sizes and the running job.
    let demo_queue: Vec<QueuedJob> = if std::env::var("SHALE_DEMO").ok().as_deref() == Some("queue")
    {
        gcode_path
            .as_ref()
            .map(|path| {
                let bytes = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
                (1..=3)
                    .map(|id| QueuedJob {
                        id,
                        name: format!("Cube plate {id}"),
                        path: path.clone(),
                        bytes,
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let demo_queue_next = demo_queue.len() as u64 + 1;
    let demo_queue_running = demo_queue.first().map(|job| job.name.clone());
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState(Mutex::new(Inner {
            catalog,
            catalog_error,
            catalog_loading,
            models,
            plates: vec![PlateStash {
                name: "plate 1".to_string(),
                ..PlateStash::default()
            }],
            active_plate: 0,
            painted: std::collections::BTreeMap::new(),
            paint_past: Vec::new(),
            paint_future: Vec::new(),
            next_id,
            instructions,
            next_instruction,
            support,
            support_model_id,
            engine,
            toolpath,
            gcode_path,
            queue: demo_queue,
            queue_next_id: demo_queue_next,
            queue_run: shale_core::queue::QueueRun::new(),
            printer_id: None,
            queue_running: demo_queue_running,
            slicing: false,
            cancel_import: import_cancel,
            engine_message,
            host_config: None,
        })))
        .manage(PrinterHost::default())
        // The profile library reader hands its result back here rather than to a
        // command, so no command ever blocks on the disk. When it lands, the
        // window is told and asks for the printer list again.
        .setup(move |app| {
            // A demo recording should fit the screen it is filmed on: the normal
            // window is 1440x900, which is larger than this laptop's desktop at
            // 200% scaling, and a window that hangs off the edge films badly.
            if std::env::var("SHALE_DEMO").is_ok() {
                if let Some(window) = app.get_webview_window("main") {
                    if let Ok(Some(monitor)) = window.current_monitor() {
                        let scale = monitor.scale_factor();
                        let width =
                            (monitor.size().width as f64 / scale - 80.0).clamp(900.0, 1440.0);
                        let height =
                            (monitor.size().height as f64 / scale - 120.0).clamp(640.0, 900.0);
                        let _ = window.set_size(tauri::LogicalSize::new(width, height));
                    }
                }
            }
            let Some(rx) = catalog_rx else {
                return Ok(());
            };
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let read = match rx.recv() {
                    Ok(read) => read,
                    Err(_) => Err("the profile library reader stopped before answering".into()),
                };
                let state = handle.state::<AppState>();
                if let Ok(mut inner) = state.0.lock() {
                    match read {
                        Ok(catalog) => {
                            inner.catalog = Some(catalog);
                            inner.catalog_error = None;
                        }
                        Err(err) => inner.catalog_error = Some(err),
                    }
                    inner.catalog_loading = false;
                }
                // The guard is not a resource with its own Drop: letting it fall
                // here ends the borrow, and the emit below cannot wake a listener
                // that wants the lock.
                let _ = state;
                let _ = handle.emit("catalogue", ());
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            secret_set,
            secret_get,
            secret_clear,
            scratch_path,
            app_status,
            startup_open,
            take_pending_open,
            preset_list,
            preset_save,
            preset_delete,
            preset_rename,
            preset_export,
            preset_import,
            cost_settings,
            set_cost_settings,
            cost_estimate,
            profile_settings,
            settings_help,
            set_engine,
            cancel_import,
            list_printers,
            list_variants,
            list_qualities,
            list_materials,
            printer_bed,
            import_model,
            model_geometry,
            duplicate_model,
            split_model,
            add_primitive,
            fill_plate,
            update_transform,
            auto_orient,
            place_on_face,
            mesh_health,
            snap_measure,
            measure_span,
            cut_model,
            boolean_model,
            calibration_tower,
            flow_coupon,
            pressure_tower,
            calibrate_slice,
            drop_model,
            arrange_models,
            restore_models,
            remove_model,
            list_instructions,
            add_instruction,
            remove_instruction,
            analyse_supports,
            propose_supports,
            apply_supports,
            current_support,
            compare_supports,
            slice_model,
            host_defaults,
            host_connect,
            host_disconnect,
            host_forget,
            host_status,
            host_forget_pin,
            host_pin,
            host_send,
            host_control,
            queue_add,
            queue_list,
            queue_remove,
            queue_clear,
            queue_arm,
            queue_observe,
            queue_plate_cleared,
            queue_stop,
            queue_run_state,
            queue_start_next,
            queue_release,
            slice_tools,
            overhang_patches,
            paint_support,
            paint_brush,
            clear_paint,
            undo_paint,
            redo_paint,
            preview_until,
            loaded_models,
            existing_slice,
            export_gcode,
            export_plate,
            set_model_settings,
            set_model_slot,
            save_project,
            open_project,
            clear_job,
            list_plates,
            add_plate,
            switch_plate,
            delete_plate,
            rename_plate,
            list_recents,
            forget_recent_project
        ]);

    // `run` would panic with a bare HRESULT when the window cannot be made, and
    // by then there is no window to put the sentence in. `build` returns the
    // error instead, so the box can say what happened before the process ends.
    //
    // `SHALE_START_FAILS=1` skips the build and hands the failure path a
    // synthetic WebView2 refusal: the real one needs a process to be holding the
    // profile at the moment of the attempt, which cannot be arranged on demand.
    // It is a diagnostic, like `SHALE_TRACE`, and does nothing unless asked for.
    let forced = std::env::var("SHALE_START_FAILS").ok();
    // `1` shows the box at once; `retry` takes the retry road first, so the
    // parent-and-child dance can be watched instead of hoped for.
    let allow_retry = forced.as_deref() != Some("1");
    let built = if forced.is_some() {
        None
    } else {
        Some(builder.build(tauri::generate_context!()))
    };
    let app = match built {
        None => {
            start_failed(
                "failed to create webview: WebView2 error: WindowsError(Error { \
                 code: HRESULT(0x800700AA), message: \"The requested resource is in \
                 use.\" })",
                allow_retry,
            );
            return;
        }
        Some(Ok(app)) => app,
        Some(Err(err)) => {
            start_failed(&format!("{err:?}"), true);
            return;
        }
    };
    app.run(|_app, _event| {});
}

/// A window that could not be made: one retry as a fresh process, then the box.
fn start_failed(detail: &str, allow_retry: bool) {
    // WebView2 usually needs a moment, not a fix: the process holding the profile
    // is finishing its exit. One retry as a fresh process covers that, and
    // `SHALE_START_RETRIED` stops the child retrying again, so a real problem
    // shows the box instead of a loop.
    let in_use = detail.to_ascii_lowercase().contains("0x800700aa");
    let retried = std::env::var_os("SHALE_START_RETRIED").is_some();
    if in_use && !retried && allow_retry {
        if std::env::var_os("SHALE_TRACE").is_some() {
            eprintln!("[shale] the window profile is still held: {detail}");
            eprintln!("[shale] retrying once in a second and a half");
        }
        std::thread::sleep(std::time::Duration::from_millis(1_500));
        // The mutex is this process's; the child has to be able to take it.
        instance::release();
        let exe = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("shale-desktop.exe"));
        let spawned = std::process::Command::new(exe)
            .env("SHALE_START_RETRIED", "1")
            .spawn();
        if spawned.is_ok() {
            return;
        }
    }
    instance::say_start_failed(detail);
}

#[cfg(test)]
mod tests {
    use super::{boolean_refusal, BOOLEAN_CAP};
    use shale_host::starts_with_machine_sequence;

    #[test]
    fn a_boolean_refusal_says_the_number_the_reason_and_the_way_round() {
        let refused = boolean_refusal(325_000, "Cut, or boolean fewer parts at a time.");
        assert!(
            refused.contains("325000"),
            "the count the app measured: {refused}"
        );
        assert!(
            refused.contains(&BOOLEAN_CAP.to_string()),
            "the cap: {refused}"
        );
        assert!(
            refused.contains("more than Shale will attempt"),
            "the sentence says why: {refused}"
        );
        assert!(
            !refused.contains("minutes"),
            "the library no longer takes minutes at 32k, so the sentence must not say it does: {refused}"
        );
        assert!(
            !refused.to_lowercase().contains("memory"),
            "the cap is not a memory limit and must not claim to be: {refused}"
        );
        assert!(
            refused.contains("Cut, or boolean fewer parts"),
            "the way round: {refused}"
        );
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("shale-gate-{name}.gcode"));
        path
    }

    #[test]
    fn a_machine_banner_is_recognised_and_a_marlin_start_is_not() {
        let bambu = temp("bambu");
        std::fs::write(
            &bambu,
            ";===== machine: A1 =========================\nM620 S0A\nG28 Z P0 T140\nG1 X10 Y10\n",
        )
        .unwrap();
        assert!(starts_with_machine_sequence(&bambu));

        let cura = temp("cura");
        std::fs::write(
            &cura,
            ";FLAVOR:Marlin\n;TIME:1234\nM140 S60\nM104 S210\nG28 ;Start GCode\n",
        )
        .unwrap();
        assert!(
            !starts_with_machine_sequence(&cura),
            "a Cura start must not count as a machine's own"
        );

        let missing = temp("missing");
        let _ = std::fs::remove_file(&missing);
        assert!(
            !starts_with_machine_sequence(&missing),
            "a file that cannot be read is not a machine sequence"
        );
    }
}
