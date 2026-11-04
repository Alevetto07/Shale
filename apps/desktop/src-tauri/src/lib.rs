//! Tauri shell. All slicing behaviour lives in `shale-core`; this file only
//! holds window state and translates commands into that API.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use shale_core::{
    compare_support, drop_to_bed, forget_recent, load_mesh_reporting, load_mesh_with,
    load_named_bytes, load_recents, now_ms, overlays, place_on_bed, propose, read_project,
    remember_recent, set_enabled, slice, Catalog, CustomInstruction, Engine, EngineBackend,
    ImportReport, MaterialInfo, PlacedModel, PrinterInfo, ProjectData, ProjectModel, QualityInfo,
    RecentEntry, ResourcePaths, StoredInstruction, SupportOverlay, SupportParams, SupportPlan,
    TessellationTolerance, Toolpath, Transform, TriangleMesh, UserSettings, VariantInfo,
};
use tauri::{AppHandle, Emitter, Manager, State};

struct Inner {
    catalog: Option<Catalog>,
    catalog_error: Option<String>,
    /// The profile library is read on a worker thread while the window comes
    /// up. Commands that need it wait here once, then keep the result. Reading
    /// it up front cost two and a half seconds of blank window on a machine
    /// with both Cura and Bambu Studio installed.
    catalog_rx: Option<std::sync::mpsc::Receiver<Result<Catalog, String>>>,
    models: Vec<LoadedModel>,
    next_id: u32,
    instructions: Vec<(u32, CustomInstruction)>,
    next_instruction: u32,
    support: Option<SupportPlan>,
    support_model_id: Option<u32>,
    engine: Engine,
    toolpath: Option<Toolpath>,
    gcode_path: Option<PathBuf>,
    slicing: bool,
    cancel_import: Arc<AtomicBool>,
    engine_message: String,
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
    warning_count: usize,
}

#[derive(Serialize)]
struct SliceSummary {
    layer_count: usize,
    layers: Vec<LayerMark>,
    print_time_seconds: Option<f64>,
    warning_count: usize,
    gcode_path: String,
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
}

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Inner>, String> {
    state
        .0
        .lock()
        .map_err(|_| "Shale state was poisoned".to_string())
}

/// Lend out the catalogue, waiting for the background read the first time a
/// command asks for it.
fn catalog(inner: &mut Inner) -> Result<&Catalog, String> {
    if inner.catalog.is_none() {
        if let Some(rx) = inner.catalog_rx.take() {
            match rx.recv() {
                Ok(Ok(catalog)) => {
                    inner.catalog = Some(catalog);
                    inner.catalog_error = None;
                }
                Ok(Err(err)) => inner.catalog_error = Some(err),
                Err(_) => {
                    inner.catalog_error =
                        Some("the profile library reader stopped before answering".into());
                }
            }
        }
    }
    inner.catalog.as_ref().ok_or_else(|| {
        inner
            .catalog_error
            .clone()
            .unwrap_or_else(|| "Resources are not loaded".into())
    })
}

#[tauri::command]
fn app_status(state: State<AppState>) -> Result<Status, String> {
    let mut inner = lock(&state)?;
    let loaded = catalog(&mut inner).is_ok();
    let resources_message = inner
        .catalog_error
        .clone()
        .unwrap_or_else(|| "Printer resources are loaded.".into());
    let printer_count = inner.catalog.as_ref().map(|c| c.printers().len()).unwrap_or(0);
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
        let generic = a.id.starts_with("generic_").cmp(&b.id.starts_with("generic_"));
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
) -> Result<ModelView, String> {
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
    let imported = tauri::async_runtime::spawn_blocking(move || {
        let source_bytes = fs::read(&load_path)?;
        let imported = load_mesh_reporting(
            &load_path,
            &tolerance,
            &mut |fraction, message| {
                let _ = app_progress.emit(
                    "import-progress",
                    serde_json::json!({ "fraction": fraction, "message": message }),
                );
            },
            &cancel,
        )?;
        Ok::<_, shale_core::Error>((source_bytes, imported))
    })
    .await
    .map_err(|err| format!("import task failed: {err}"))?
    .map_err(|err| err.to_string())?;
    let (source_bytes, imported) = imported;
    let mesh = imported.mesh;
    let report = imported.report;
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
    let transform = match placed {
        Some(resolved) => place_on_bed(
            &mesh,
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_center_is_zero,
        ),
        None => {
            let mut transform = Transform::default();
            drop_to_bed(&mesh, &mut transform);
            transform
        }
    };
    let id = inner.next_id;
    inner.next_id += 1;
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model")
        .to_string();
    let settings = std::collections::BTreeMap::new();
    let view = view_of(id, &name, &transform, &report, &settings);
    inner.models.push(LoadedModel {
        id,
        name,
        mesh,
        transform,
        report,
        source_bytes,
        visible: true,
        settings: std::collections::BTreeMap::new(),
    });
    Ok(view)
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
            )
        })
        .collect())
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

#[tauri::command]
async fn slice_model(
    app: AppHandle,
    state: State<'_, AppState>,
    printer_id: String,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    settings: UserSettings,
) -> Result<SliceSummary, String> {
    let (catalog_paths, engine, placed, output) = {
        let mut inner = lock(&state)?;
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
            })
            .collect();
        let output = std::env::temp_dir().join("shale").join("slice.gcode");
        (
            catalog(&mut inner).ok().map(|c| c.paths.clone()),
            inner.engine.clone(),
            placed,
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
    let app_progress = app.clone();
    let resolved_and_models = tauri::async_runtime::spawn_blocking(move || {
        slice(
            &catalog,
            &engine,
            &printer_id,
            variant_name,
            quality_type,
            material_id,
            settings,
            &placed,
            &instructions,
            support.as_ref(),
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
        print_time_seconds: outcome.print_time_seconds,
        warning_count: outcome.resolved.warning_count,
        gcode_path: outcome.gcode_path.display().to_string(),
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
            )
        })
        .collect())
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
        print_time_seconds: None,
        warning_count: 0,
        gcode_path: path.display().to_string(),
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
        .map(|model| (model.name.as_str(), shale_core::mesh::bake(&model.mesh, &model.transform)))
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
            let parts: Vec<(&str, &TriangleMesh)> = visible
                .iter()
                .map(|(name, mesh)| (*name, mesh))
                .collect();
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
        warning_count: resolved.warning_count,
    }
}

fn view_of(
    id: u32,
    name: &str,
    transform: &Transform,
    report: &ImportReport,
    settings: &std::collections::BTreeMap<String, String>,
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
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
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
    // A plain demo model is imported from the window so a large file does not
    // freeze startup. Preload only when a slice or a support plan is requested.
    let preload = std::env::var("SHALE_DEMO_SLICE").ok().as_deref() == Some("1")
        || std::env::var("SHALE_DEMO_SUPPORTS").ok().as_deref() == Some("1");
    if !preload {
        return (Vec::new(), Vec::new(), None, None);
    }
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
            Some("generic_pla".into()),
            UserSettings {
                layer_height: Some(0.2),
                infill_sparse_density: Some(20.0),
                support_enable: Some(false),
                material_print_temperature: Some(200.0),
                material_bed_temperature: Some(60.0),
                extra: Vec::new(),
            },
            &[PlacedModel {
                name: name.clone(),
                mesh: mesh.clone(),
                transform: transform.clone(),
                settings: Vec::new(),
            }],
            &[pause],
            None,
            output,
            &mut |_| {},
        ) {
            gcode_path = Some(outcome.gcode_path);
            toolpath = Some(outcome.toolpath);
        }
    }
    (
        vec![LoadedModel {
            id: 1,
            name,
            mesh,
            transform,
            report,
            source_bytes: fs::read(std::path::Path::new(&path)).unwrap_or_default(),
            visible: true,
            settings: std::collections::BTreeMap::new(),
        }],
        instructions,
        toolpath,
        gcode_path,
    )
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
    settings: UserSettings,
    chord_mm: f64,
    turn_deg: f64,
    thumbnail: Vec<u8>,
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

fn remember(app: &AppHandle, path: &str, name: &str, printer: &str) -> Result<Vec<RecentEntry>, String> {
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

#[tauri::command]
fn clear_job(state: State<AppState>) -> Result<(), String> {
    let mut inner = lock(&state)?;
    inner.models.clear();
    inner.next_id = 1;
    inner.instructions.clear();
    inner.next_instruction = 1;
    inner.support = None;
    inner.support_model_id = None;
    inner.toolpath = None;
    inner.gcode_path = None;
    Ok(())
}

#[tauri::command]
fn save_project(
    app: AppHandle,
    state: State<AppState>,
    project: SaveRequest,
) -> Result<Vec<RecentEntry>, String> {
    let inner = lock(&state)?;
    let support_model = inner.support_model_id.and_then(|id| {
        inner
            .models
            .iter()
            .position(|model| model.id == id)
    });
    let mut support = inner.support.clone();
    if support_model.is_none() {
        support = None;
    }
    let models = inner
        .models
        .iter()
        .filter(|model| model.visible)
        .map(|model| {
            if model.source_bytes.is_empty() {
                return Err(format!(
                    "Cannot save {}: the original file is missing.",
                    model.name
                ));
            }
            Ok(ProjectModel {
                name: model.name.clone(),
                bytes: model.source_bytes.clone(),
                transform: model.transform.clone(),
                settings: model.settings.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let data = ProjectData {
        name: project.name.clone(),
        printer_id: project.printer_id,
        printer_label: project.printer_label.clone(),
        variant_name: project.variant_name,
        quality_type: project.quality_type,
        material_id: project.material_id,
        settings: project.settings,
        chord_mm: project.chord_mm,
        turn_deg: project.turn_deg,
        models,
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
    remember(&app, &project.path, &project.name, &project.printer_label)
}

#[tauri::command]
async fn open_project(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<OpenedJob, String> {
    let file = PathBuf::from(&path);
    let loaded = tauri::async_runtime::spawn_blocking(move || {
        let project = read_project(&file).map_err(|err| err.to_string())?;
        let tolerance = TessellationTolerance::new(
            if project.data.chord_mm > 0.0 { project.data.chord_mm } else { 0.1 },
            if project.data.turn_deg > 0.0 { project.data.turn_deg } else { 15.0 },
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
            ));
        }
        Ok::<_, String>((project, built))
    })
    .await
    .map_err(|err| format!("open task failed: {err}"))??;
    let (project, built) = loaded;
    let mut inner = lock(&state)?;
    inner.models.clear();
    inner.instructions.clear();
    inner.support = None;
    inner.support_model_id = None;
    inner.toolpath = None;
    inner.gcode_path = None;
    inner.next_id = 1;
    inner.next_instruction = 1;
    let mut views = Vec::new();
    for (name, bytes, transform, imported, settings) in built {
        let id = inner.next_id;
        inner.next_id += 1;
        let view = view_of(id, &name, &transform, &imported.report, &settings);
        inner.models.push(LoadedModel {
            id,
            name,
            mesh: imported.mesh,
            transform,
            report: imported.report,
            source_bytes: bytes,
            visible: true,
            settings,
        });
        views.push(view);
    }
    for instruction in project.data.instructions.clone() {
        let id = inner.next_instruction;
        inner.next_instruction += 1;
        inner.instructions.push((id, instruction));
    }
    if let Some(index) = project.data.support_model {
        if let Some(model) = inner.models.get(index) {
            inner.support_model_id = Some(model.id);
            inner.support = project.data.support.clone();
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

pub fn run() {
    let started = std::time::Instant::now();
    let engine = Engine::from_env();
    // The profile library is read on a worker thread: two and a half seconds of
    // blank window on a machine with Cura and Bambu Studio installed is not a
    // good first impression, and a slow or broken library should never stop the
    // window from opening. Commands wait for it through `catalog()`.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let read = match ResourcePaths::discover() {
            Ok(paths) => match Catalog::open(paths) {
                Ok(catalog) => Ok(catalog),
                Err(err) => Err(err.to_string()),
            },
            Err(err) => Err(err.to_string()),
        };
        let _ = tx.send(read);
    });
    // The demo state is dev-only and slices at startup, so it waits here.
    let demo_wanted = std::env::var_os("SHALE_DEMO_MODEL").is_some();
    let (catalog, catalog_error, catalog_rx, demo_catalog) = if demo_wanted {
        match rx.recv() {
            Ok(Ok(catalog)) => (Some(catalog), None, None, true),
            Ok(Err(err)) => (None, Some(err), None, false),
            Err(_) => (None, Some("the profile library reader stopped".into()), None, false),
        }
    } else {
        (None, None, Some(rx), false)
    };
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState(Mutex::new(Inner {
            catalog,
            catalog_error,
            catalog_rx,
            models,
            next_id,
            instructions,
            next_instruction,
            support,
            support_model_id,
            engine,
            toolpath,
            gcode_path,
            slicing: false,
            cancel_import: import_cancel,
            engine_message,
        })))
        .invoke_handler(tauri::generate_handler![
            app_status,
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
            update_transform,
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
            preview_until,
            loaded_models,
            existing_slice,
            export_gcode,
            export_plate,
            set_model_settings,
            save_project,
            open_project,
            clear_job,
            list_recents,
            forget_recent_project
        ])
        .run(tauri::generate_context!())
        .expect("Shale failed to start");
}
