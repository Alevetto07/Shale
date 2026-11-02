//! Tauri shell. All slicing behaviour lives in `shale-core`; this file only
//! holds window state and translates commands into that API.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use shale_core::{
    compare_support, drop_to_bed, load_mesh, overlays, place_on_bed, propose, set_enabled, slice,
    Catalog, CliEngine, CustomInstruction, MaterialInfo, PlacedModel, PrinterInfo, QualityInfo,
    ResourcePaths, StoredInstruction, SupportOverlay, SupportParams, SupportPlan, Toolpath,
    Transform, TriangleMesh, UserSettings, VariantInfo,
};
use tauri::{AppHandle, Emitter, State};

struct Inner {
    catalog: Option<Catalog>,
    catalog_error: Option<String>,
    models: Vec<LoadedModel>,
    next_id: u32,
    instructions: Vec<(u32, CustomInstruction)>,
    next_instruction: u32,
    support: Option<SupportPlan>,
    engine: CliEngine,
    toolpath: Option<Toolpath>,
    gcode_path: Option<PathBuf>,
    slicing: bool,
}

struct LoadedModel {
    id: u32,
    name: String,
    mesh: TriangleMesh,
    transform: Transform,
}

struct AppState(Mutex<Inner>);

#[derive(Serialize)]
struct Status {
    resources_ok: bool,
    resources_message: String,
    engine_path: String,
    engine_ok: bool,
    printer_count: usize,
}

#[derive(Serialize)]
struct ModelView {
    id: u32,
    name: String,
    positions: Vec<f32>,
    transform: Transform,
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

#[tauri::command]
fn app_status(state: State<AppState>) -> Result<Status, String> {
    let inner = lock(&state)?;
    let engine_ok = inner.engine.available();
    Ok(Status {
        resources_ok: inner.catalog.is_some(),
        resources_message: inner
            .catalog_error
            .clone()
            .unwrap_or_else(|| "Cura resources are loaded.".into()),
        engine_path: inner.engine.program.display().to_string(),
        engine_ok,
        printer_count: inner
            .catalog
            .as_ref()
            .map(|c| c.printers().len())
            .unwrap_or(0),
    })
}

#[tauri::command]
fn set_engine(
    state: State<AppState>,
    path: String,
    lib_dir: Option<String>,
) -> Result<bool, String> {
    let mut inner = lock(&state)?;
    inner.engine.program = PathBuf::from(path);
    inner.engine.lib_dir = lib_dir.filter(|value| !value.is_empty()).map(PathBuf::from);
    Ok(inner.engine.available())
}

#[tauri::command]
fn list_printers(state: State<AppState>) -> Result<Vec<PrinterInfo>, String> {
    let inner = lock(&state)?;
    let catalog = inner.catalog.as_ref().ok_or_else(|| {
        inner
            .catalog_error
            .clone()
            .unwrap_or_else(|| "Resources are not loaded".into())
    })?;
    Ok(catalog.printers())
}

#[tauri::command]
fn list_variants(state: State<AppState>, printer_id: String) -> Result<Vec<VariantInfo>, String> {
    let inner = lock(&state)?;
    let catalog = catalog_of(&inner)?;
    Ok(catalog.variants_for(&printer_id))
}

#[tauri::command]
fn list_qualities(state: State<AppState>, printer_id: String) -> Result<Vec<QualityInfo>, String> {
    let inner = lock(&state)?;
    let catalog = catalog_of(&inner)?;
    Ok(catalog.qualities_for(&printer_id))
}

#[tauri::command]
fn list_materials(state: State<AppState>, printer_id: String) -> Result<Vec<MaterialInfo>, String> {
    let inner = lock(&state)?;
    let catalog = catalog_of(&inner)?;
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
    let mut materials: Vec<MaterialInfo> = catalog
        .materials_for(diameter)
        .into_iter()
        .filter(|material| {
            material.id.starts_with("generic_")
                || material.id.contains("pla")
                    && material.id.chars().filter(|ch| *ch == '_').count() <= 2
        })
        .collect();
    // Prefer the short generic profiles at the top of a long library.
    materials.sort_by(|a, b| a.id.cmp(&b.id));
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
    let inner = lock(&state)?;
    let catalog = catalog_of(&inner)?;
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
fn import_model(
    state: State<AppState>,
    path: String,
    printer_id: Option<String>,
    variant_name: Option<String>,
    quality_type: Option<String>,
) -> Result<ModelView, String> {
    let mesh = load_mesh(PathBuf::from(&path).as_path()).map_err(|err| err.to_string())?;
    let mut inner = lock(&state)?;
    let transform = if let (Some(catalog), Some(printer_id)) = (&inner.catalog, printer_id) {
        match catalog.resolve(&shale_core::ResolveRequest {
            printer_id,
            variant_name,
            quality_type,
            material_id: None,
            intent_category: None,
            settings: UserSettings::default(),
        }) {
            Ok(resolved) => place_on_bed(
                &mesh,
                resolved.summary.machine_width,
                resolved.summary.machine_depth,
                resolved.summary.machine_center_is_zero,
            ),
            Err(_) => {
                let mut transform = Transform::default();
                drop_to_bed(&mesh, &mut transform);
                transform
            }
        }
    } else {
        let mut transform = Transform::default();
        drop_to_bed(&mesh, &mut transform);
        transform
    };
    let id = inner.next_id;
    inner.next_id += 1;
    let name = PathBuf::from(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model")
        .to_string();
    let view = view_of(id, &name, &mesh, &transform);
    inner.models.push(LoadedModel {
        id,
        name,
        mesh,
        transform,
    });
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

#[tauri::command]
fn remove_model(state: State<AppState>, id: u32) -> Result<(), String> {
    let mut inner = lock(&state)?;
    inner.models.retain(|model| model.id != id);
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
        })
        .collect();
    let comparison = {
        let catalog = catalog_of(&inner)?;
        compare_support(
            catalog,
            &inner.engine,
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
fn slice_model(
    app: AppHandle,
    state: State<AppState>,
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
        if inner.models.is_empty() {
            return Err("Import a model first".into());
        }
        let Some(catalog) = inner.catalog.as_ref() else {
            return Err(inner
                .catalog_error
                .clone()
                .unwrap_or_else(|| "Resources are not loaded".into()));
        };
        // Catalog is not cloned; resolve inside the lock would block the UI
        // for a few milliseconds, then the engine runs after we release it.
        let _ = catalog;
        inner.slicing = true;
        let placed: Vec<PlacedModel> = inner
            .models
            .iter()
            .map(|model| PlacedModel {
                name: model.name.clone(),
                mesh: model.mesh.clone(),
                transform: model.transform.clone(),
            })
            .collect();
        let output = std::env::temp_dir().join("shale").join("slice.gcode");
        (
            inner.catalog.as_ref().map(|c| c.paths.clone()),
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
    let resolved_and_models = {
        let inner = lock(&state)?;
        let catalog = catalog_of(&inner)?;
        // We only need the catalog for resolve + slice. slice() takes &Catalog.
        // Keep the lock for the whole slice so we don't deep-copy the catalog.
        // Emit progress from this thread; Tauri's command pool is not the UI thread.
        let _ = paths;
        let instructions: Vec<CustomInstruction> = inner
            .instructions
            .iter()
            .map(|(_, instruction)| instruction.clone())
            .collect();
        let outcome = slice(
            catalog,
            &engine,
            &printer_id,
            variant_name,
            quality_type,
            material_id,
            settings,
            &placed,
            &instructions,
            inner.support.as_ref(),
            output,
            &mut |progress| {
                let _ = app.emit(
                    "slice-progress",
                    serde_json::json!({
                        "fraction": progress.fraction,
                        "message": progress.message,
                    }),
                );
            },
        );
        outcome
    };

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
        .map(|model| view_of(model.id, &model.name, &model.mesh, &model.transform))
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

fn catalog_of(inner: &Inner) -> Result<&Catalog, String> {
    inner.catalog.as_ref().ok_or_else(|| {
        inner
            .catalog_error
            .clone()
            .unwrap_or_else(|| "Resources are not loaded".into())
    })
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

fn view_of(id: u32, name: &str, mesh: &TriangleMesh, transform: &Transform) -> ModelView {
    ModelView {
        id,
        name: name.to_string(),
        positions: mesh.preview_positions(80_000),
        transform: transform.clone(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
fn preload_demo(
    catalog: &Catalog,
    engine: &CliEngine,
) -> (
    Vec<LoadedModel>,
    Vec<(u32, CustomInstruction)>,
    Option<Toolpath>,
    Option<PathBuf>,
) {
    let Some(path) = std::env::var_os("SHALE_DEMO_MODEL") else {
        return (Vec::new(), Vec::new(), None, None);
    };
    let Ok(mesh) = load_mesh(std::path::Path::new(&path)) else {
        return (Vec::new(), Vec::new(), None, None);
    };
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
        }],
        instructions,
        toolpath,
        gcode_path,
    )
}

pub fn run() {
    let (catalog, catalog_error) = match ResourcePaths::discover() {
        Ok(paths) => match Catalog::open(paths) {
            Ok(catalog) => (Some(catalog), None),
            Err(err) => (None, Some(err.to_string())),
        },
        Err(err) => (None, Some(err.to_string())),
    };
    let engine = CliEngine::from_env();
    let (models, instructions, toolpath, gcode_path) = if let Some(catalog) = &catalog {
        preload_demo(catalog, &engine)
    } else {
        (Vec::new(), Vec::new(), None, None)
    };
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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState(Mutex::new(Inner {
            catalog,
            catalog_error,
            models,
            next_id,
            instructions,
            next_instruction,
            support,
            engine,
            toolpath,
            gcode_path,
            slicing: false,
        })))
        .invoke_handler(tauri::generate_handler![
            app_status,
            set_engine,
            list_printers,
            list_variants,
            list_qualities,
            list_materials,
            printer_bed,
            import_model,
            update_transform,
            drop_model,
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
            export_gcode
        ])
        .run(tauri::generate_context!())
        .expect("Shale failed to start");
}
