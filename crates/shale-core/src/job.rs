//! A slice job: resolve the profile, bake placed meshes to STL, run the engine,
//! then read the toolpath preview back out of the G-code.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::catalog::Catalog;
use crate::engine::EngineMesh;
use crate::engine::{EngineProgress, EngineRequest, SlicingEngine};
use crate::error::Error;
use crate::gcode::{self, Toolpath};
use crate::instructions::{self, CustomInstruction, GcodeFlavour};
use crate::mesh::{self, Transform, TriangleMesh};
use crate::paint;
use crate::resolver::{ResolveRequest, ResolvedSettings, UserSettings};
use crate::supports::{self, SupportPlan};
use crate::template::{self, Value, Values};

/// Bambu's machine profiles keep the machine's own start and end sequence as a
/// template beside the profile, and that sequence is where the machine wipes
/// the nozzle, levels the bed and hands over to the AMS. Cura's generic Marlin
/// start does none of it, so a Bambu machine gets its own or nothing.
///
/// `None` is a machine with no template of its own — every Cura printer, and a
/// Bambu nozzle whose profile ships none — which keeps the start it had.
fn machine_sequence(
    catalog: &Catalog,
    resolved: &ResolvedSettings,
    models: &[PlacedModel],
) -> Result<Option<(String, String)>, Error> {
    let nozzle = resolved
        .variant_name
        .split_whitespace()
        .next()
        .unwrap_or("0.4");
    let Some(gcode) = catalog.machine_gcode(&resolved.printer_id, nozzle) else {
        return Ok(None);
    };

    // Filament type is not a Cura setting: it comes from the filament profile.
    let filament_type = catalog
        .materials
        .iter()
        .find(|material| material.id == resolved.material_id)
        .and_then(|material| material.filament_type.clone())
        .or_else(|| {
            let name = resolved.material_id.to_ascii_uppercase();
            ["PLA", "PETG", "ABS", "ASA", "TPU", "PC", "PVA", "HIPS"]
                .into_iter()
                .find(|kind| name.contains(kind))
                .map(str::to_string)
        })
        .ok_or_else(|| {
            Error::message(
                "Bambu's start sequence needs the filament type, and this filament's \
                 profile does not say which one it is"
                    .to_string(),
            )
        })?;
    render_sequence(gcode, &filament_type, resolved, models).map(Some)
}

/// Bambu template names that the resolver spells in Cura's vocabulary (see
/// `docs/SETTINGS-MAP.md`).
fn bambu_cura_template_aliases() -> &'static [(&'static str, &'static str)] {
    &[
        ("default_acceleration", "acceleration_print"),
        ("outer_wall_acceleration", "acceleration_wall_0"),
        ("inner_wall_acceleration", "acceleration_wall_x"),
        ("initial_layer_acceleration", "acceleration_layer_0"),
        ("sparse_infill_acceleration", "acceleration_infill"),
        ("top_surface_acceleration", "acceleration_topbottom"),
        ("travel_acceleration", "acceleration_travel"),
        ("outer_wall_speed", "speed_wall_0"),
        ("inner_wall_speed", "speed_wall_x"),
        ("sparse_infill_speed", "speed_infill"),
        ("internal_solid_infill_speed", "speed_topbottom"),
        ("top_surface_speed", "speed_topbottom"),
        ("initial_layer_speed", "speed_layer_0"),
        ("travel_speed", "speed_travel"),
        ("travel_speed_z", "speed_z_hop"),
        ("support_speed", "speed_support"),
        ("bridge_speed", "speed_bridge"),
        ("retraction_length", "retraction_amount"),
        ("z_hop", "retraction_hop"),
        ("wall_loops", "wall_line_count"),
        ("sparse_infill_density", "infill_sparse_density"),
        ("top_shell_layers", "top_layers"),
        ("bottom_shell_layers", "bottom_layers"),
        ("layer_height", "layer_height"),
        ("initial_layer_print_height", "layer_height_0"),
    ]
}

/// Fill a machine's own start and end templates. Separated from the catalogue
/// so a test can render the real A1 sequence from the fixtures without reading
/// two thousand profiles first.
pub fn render_sequence(
    gcode: &crate::bambu::MachineGcode,
    filament_type: &str,
    resolved: &ResolvedSettings,
    models: &[PlacedModel],
) -> Result<(String, String), Error> {
    // Every resolved setting, then the per-extruder lists Bambu writes with a
    // subscript, then the names its templates use that Cura spells differently.
    let mut values = Values::new();
    for (key, value) in &resolved.global {
        values.set_setting(key, value);
    }
    for (key, value) in crate::bambu::template_scalars_for_slice(
        &resolved.printer_id,
        &resolved.variant_name,
        &resolved.quality_type,
        &resolved.material_id,
    ) {
        if values.get(&key).is_none() {
            values.set_setting(&key, &value);
        }
    }
    for (key, list) in crate::bambu::template_lists_for_slice(
        &resolved.printer_id,
        &resolved.variant_name,
        &resolved.quality_type,
        &resolved.material_id,
    ) {
        if values.get(&key).is_none() {
            values.set_list(&key, list);
        }
    }
    if values.get("nozzle_diameter_at_nozzle_id").is_none() {
        if let Some(list) = values.get("nozzle_diameter").cloned() {
            values.set("nozzle_diameter_at_nozzle_id", list);
        } else if let Some(text) = resolved.global.get("machine_nozzle_size") {
            let diam = text.parse::<f64>().unwrap_or(0.4);
            let slots = resolved.extruders.len().max(2);
            values.set_list(
                "nozzle_diameter_at_nozzle_id",
                vec![Value::Num(diam); slots],
            );
        }
    }
    for (name, default) in [
        ("current_nozzle_id", 0.0),
        ("current_extruder", 0.0),
        ("current_filament_id", 0.0),
        ("initial_nozzle_id", 0.0),
        ("initial_no_support_extruder", 0.0),
        ("initial_no_support_filament_id", 0.0),
        ("initial_no_support_hotend", 0.0),
        ("cooling_filter_enabled", 0.0),
        ("long_retraction_when_ec", 0.0),
        ("long_retraction_when_cut", 0.0),
        ("next_hotend", 0.0),
        ("next_filament_id", 0.0),
        ("toolchange_count", 0.0),
        ("wipe_tower_center_pos_valid", 0.0),
        ("hold_chamber_temp_for_flat_print", 0.0),
        ("support_chamber_temp_control", 0.0),
        ("support_air_filtration", 0.0),
        ("layer_num", 0.0),
    ] {
        if values.get(name).is_none() {
            values.set(name, default);
        }
    }
    if values.get("activate_air_filtration").is_none() {
        let slots = resolved.extruders.len().max(1);
        values.set_list("activate_air_filtration", vec![Value::Num(0.0); slots]);
    }
    let tool_slots = resolved.extruders.len().max(2);
    if values.get("filament_map").is_none() {
        values.set_list("filament_map", vec![Value::Num(0.0); tool_slots]);
    }
    if values.get("first_filaments").is_none() {
        values.set_list("first_filaments", vec![Value::Num(0.0); tool_slots]);
    }
    if values.get("first_non_support_filaments").is_none() {
        values.set_list(
            "first_non_support_filaments",
            vec![Value::Num(-1.0); tool_slots],
        );
    }
    if values.get("filament_diameter").is_none() {
        let diam = resolved
            .global
            .get("material_diameter")
            .and_then(|text| text.parse::<f64>().ok())
            .unwrap_or(1.75);
        values.set_list("filament_diameter", vec![Value::Num(diam); tool_slots]);
    }
    if values.get("filament_max_volumetric_speed").is_none() {
        let default_speed = resolved
            .global
            .get("material_max_volumetric_speed")
            .and_then(|text| text.parse::<f64>().ok())
            .unwrap_or(15.0);
        values.set_list(
            "filament_max_volumetric_speed",
            vec![Value::Num(default_speed); tool_slots],
        );
    }
    let mut lists: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();
    for extruder in &resolved.extruders {
        for (key, value) in extruder {
            lists
                .entry(key.clone())
                .or_default()
                .push(template::value_of(value));
        }
    }
    for (key, list) in lists {
        values.set_list(&key, list);
    }

    // `nozzle_temperature*` is Cura's `material_print_temperature*` under
    // Bambu's name, once per extruder.
    for (bambu, cura) in [
        ("nozzle_temperature", "material_print_temperature"),
        (
            "nozzle_temperature_initial_layer",
            "material_print_temperature_layer_0",
        ),
    ] {
        let global = resolved
            .global
            .get(cura)
            .or_else(|| resolved.global.get("material_print_temperature"));
        let mut list: Vec<Value> = Vec::new();
        for extruder in &resolved.extruders {
            let text = extruder
                .get(cura)
                .or_else(|| extruder.get("material_print_temperature"))
                .or(global);
            match text {
                Some(text) => list.push(template::value_of(text)),
                None => {
                    list.clear();
                    break;
                }
            }
        }
        if list.is_empty() {
            list = global
                .map(|text| vec![template::value_of(text)])
                .unwrap_or_default();
        }
        // Left unset when nothing resolved: the render then names the missing
        // name and the slice stops, rather than starting a machine at 0 degrees.
        if !list.is_empty() {
            values.set_list(bambu, list);
        }
    }
    // Orca/Bambu machine and filament templates also quote `bed_temperature*`
    // rather than Cura's `material_bed_temperature*`.
    for (bambu, cura) in [
        ("bed_temperature", "material_bed_temperature"),
        (
            "bed_temperature_initial_layer",
            "material_bed_temperature_layer_0",
        ),
    ] {
        let global = resolved
            .global
            .get(cura)
            .or_else(|| resolved.global.get("material_bed_temperature"));
        let mut list: Vec<Value> = Vec::new();
        for extruder in &resolved.extruders {
            let text = extruder
                .get(cura)
                .or_else(|| extruder.get("material_bed_temperature"))
                .or(global);
            match text {
                Some(text) => list.push(template::value_of(text)),
                None => {
                    list.clear();
                    break;
                }
            }
        }
        if list.is_empty() {
            list = global
                .map(|text| vec![template::value_of(text)])
                .unwrap_or_default();
        }
        if !list.is_empty() {
            values.set_list(bambu, list);
        }
    }
    // The machine's own bed-temperature name for the first layer.
    if values.get("bed_temperature_initial_layer_single").is_none() {
        if let Some(text) = resolved
            .global
            .get("material_bed_temperature_layer_0")
            .or_else(|| resolved.global.get("material_bed_temperature"))
        {
            values.set_setting("bed_temperature_initial_layer_single", text);
        }
    }
    // The purge rate and the calibration feed are quoted as volumetric speeds,
    // which is how a Bambu filament profile writes its limit.
    if values.get("flush_volumetric_speeds").is_none() {
        if let Some(text) = resolved.global.get("filament_max_volumetric_speed") {
            let rate = template::value_of(text);
            let slots = resolved.extruders.len().max(1);
            values.set_list("flush_volumetric_speeds", vec![rate; slots]);
        }
    }
    // The A1 wipes and calibrates at the outer wall's volumetric rate; with only
    // Cura's settings to hand, the filament's limit is the honest stand-in.
    if values.get("outer_wall_volumetric_speed").is_none() {
        if let Some(text) = resolved.global.get("filament_max_volumetric_speed") {
            values.set_setting("outer_wall_volumetric_speed", text);
        }
    }

    // The first layer's temperature is also what the flush uses.
    if values.get("flush_temperatures").is_none() {
        if let Some(list) = values.get("nozzle_temperature_initial_layer") {
            let list = list.clone();
            values.set("flush_temperatures", list);
        }
    }

    if values.get("filament_type").is_none() {
        let slots = resolved.extruders.len().max(1);
        values.set_list(
            "filament_type",
            vec![Value::Text(filament_type.to_string()); slots],
        );
    }

    // Cura's spellings.
    if values.get("spiral_mode").is_none() {
        let spiral = resolved
            .global
            .get("magic_spiralize")
            .map(|text| text == "true" || text == "True")
            .unwrap_or(false);
        values.set("spiral_mode", Value::Flag(spiral));
    }
    if values.get("print_sequence").is_none() {
        let sequence = resolved
            .global
            .get("print_sequence")
            .cloned()
            .unwrap_or_else(|| "all_at_once".into());
        let sequence = match sequence.as_str() {
            "one_at_a_time" => "by object",
            _ => "all",
        };
        values.set("print_sequence", Value::Text(sequence.to_string()));
    }
    for name in ["initial_extruder", "initial_no_support_extruder"] {
        if values.get(name).is_none() {
            values.set(name, 0);
        }
    }
    if values.get("nozzle_diameter").is_none() {
        if let Some(text) = resolved.global.get("machine_nozzle_size") {
            values.set_setting("nozzle_diameter", text);
        }
    }
    if values.get("curr_bed_type").is_none() {
        // The A1 ships with a textured PEI plate, and its template picks the bed
        // scan routine by this name.
        values.set("curr_bed_type", Value::Text("Textured PEI Plate".into()));
    }
    if values.get("scan_first_layer").is_none() {
        let scan = resolved
            .global
            .get("scan_first_layer")
            .map(String::as_str)
            .unwrap_or("0");
        values.set_setting("scan_first_layer", scan);
    }
    for &(bambu, cura) in bambu_cura_template_aliases() {
        if values.get(bambu).is_none() {
            let text = resolved.global.get(cura).or_else(|| {
                resolved
                    .extruders
                    .first()
                    .and_then(|extruder| extruder.get(cura))
            });
            if let Some(text) = text {
                values.set_setting(bambu, text);
            }
        }
    }

    // Where the first layer sits, which the wipe and the bed scan both use.
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for model in models {
        let baked = mesh::bake(&model.mesh, &model.transform);
        let (low, high) = mesh::bounds(&baked);
        for axis in 0..3 {
            min[axis] = min[axis].min(low[axis]);
            max[axis] = max[axis].max(high[axis]);
        }
    }
    if min[0].is_finite() {
        values.set("max_layer_z", max[2]);
        values.set_list(
            "first_layer_print_min",
            vec![Value::Num(min[0]), Value::Num(min[1])],
        );
        values.set_list(
            "first_layer_print_size",
            vec![Value::Num(max[0] - min[0]), Value::Num(max[1] - min[1])],
        );
        values.set_list(
            "first_layer_center_no_wipe_tower",
            vec![
                Value::Num((min[0] + max[0]) / 2.0),
                Value::Num((min[1] + max[1]) / 2.0),
            ],
        );
    }
    if values.get("max_print_z").is_none() {
        let height = if max[2].is_finite() && min[0].is_finite() {
            max[2]
        } else {
            resolved
                .global
                .get("machine_height")
                .and_then(|text| text.parse::<f64>().ok())
                .unwrap_or(resolved.summary.machine_height)
        };
        values.set("max_print_z", height);
    }
    if values.get("max_layer_z").is_none() {
        let z = if max[2].is_finite() && min[0].is_finite() {
            max[2]
        } else {
            0.0
        };
        values.set("max_layer_z", z);
    }

    template::ensure_runtime_scalar_defaults(&gcode.start, &mut values)?;
    template::ensure_runtime_scalar_defaults(&gcode.end, &mut values)?;
    let start = template::render(&gcode.start, &values)?;
    let end = template::render(&gcode.end, &values)?;
    Ok((start, end))
}

#[derive(Clone, Debug)]
pub struct PlacedModel {
    pub name: String,
    pub mesh: TriangleMesh,
    pub transform: Transform,
    /// Per-part overrides. The engine reads these per object, so one part can
    /// print with six walls while its neighbour keeps two.
    pub settings: Vec<(String, String)>,
    /// Which spool in the AMS prints this part. `None` is the machine's own
    /// extruder, and that is also what slot 0 resolves to.
    pub slot: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SliceOutcome {
    pub gcode_path: PathBuf,
    pub resolved: ResolvedSettings,
    pub toolpath: Toolpath,
    pub print_time_seconds: Option<f64>,
    pub filament_metres: f64,
    pub flavour: GcodeFlavour,
    pub log_tail: String,
    /// The machine's own start and end sequence, filled in, was used for this
    /// slice. `false` is Cura's generic Marlin start.
    pub machine_gcode: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonplanar_report: Option<shale_nonplanar::NonplanarReport>,
    /// [`NonplanarOptions::defer_post_pass`]: raw CuraEngine moves before inverse.
    #[serde(skip)]
    pub engine_gcode: Option<String>,
    /// [`NonplanarOptions::defer_post_pass`]: undeformed-mesh baseline from the same job.
    #[serde(skip)]
    pub planar_baseline_gcode: Option<String>,
    /// [`NonplanarOptions::defer_post_pass`]: the exact field the sliced meshes were
    /// deformed with, so a deferred post-pass maps with it instead of rebuilding one.
    #[serde(skip)]
    pub nonplanar_context: Option<shale_nonplanar::NonplanarContext>,
}

/// Cura stores `machine_max_feedrate_*` in mm/s; F words use mm/min.
fn machine_max_feedrate_mm_per_min(resolved: &ResolvedSettings) -> f64 {
    for key in ["machine_max_feedrate_x", "machine_max_speed_x"] {
        if let Some(raw) = resolved.global.get(key) {
            if let Ok(v) = raw.parse::<f64>() {
                return (v * 60.0).max(600.0);
            }
        }
    }
    (resolved.summary.speed_print * 60.0).max(600.0)
}

pub fn slice(
    catalog: &Catalog,
    engine: &dyn SlicingEngine,
    printer_id: &str,
    variant_name: Option<String>,
    quality_type: Option<String>,
    // One filament per AMS slot, in slot order. `None` leaves the printer to pick.
    materials: &[Option<String>],
    settings: UserSettings,
    models: &[PlacedModel],
    instructions: &[CustomInstruction],
    support: Option<&SupportPlan>,
    // Faces painted as needing support. Each patch becomes a column of prisms
    // under the part, handed to the engine as a `support_mesh`.
    painted: &[paint::PaintedPatch],
    output_path: PathBuf,
    on_progress: &mut dyn FnMut(EngineProgress),
) -> Result<SliceOutcome, Error> {
    if models.is_empty() {
        return Err(Error::message("import a model before slicing"));
    }
    let output_path = crate::engine::absolutize_path(&output_path)?;
    on_progress(EngineProgress::new(0.02, "Resolving printer settings"));
    let request_for = |material: Option<String>| ResolveRequest {
        printer_id: printer_id.to_string(),
        variant_name: variant_name.clone(),
        quality_type: quality_type.clone(),
        material_id: material,
        intent_category: None,
        settings: settings.clone(),
    };
    let mut resolved = catalog.resolve(&request_for(materials.first().cloned().flatten()))?;

    // Each slot is its own material, so each slot gets its own resolve: that is
    // what gives the engine a second nozzle temperature and a second filament.
    // The plate's non-material settings still come from slot 0.
    let slot_count = models
        .iter()
        .map(|model| model.slot.unwrap_or(0))
        .max()
        .unwrap_or(0)
        + 1;
    if slot_count > 1 {
        let mut extruders = vec![resolved.extruders.first().cloned().unwrap_or_default()];
        for index in 1..slot_count {
            let material = materials
                .get(index)
                .cloned()
                .flatten()
                .or_else(|| materials.first().cloned().flatten());
            let extra = catalog.resolve(&request_for(material))?;
            let mut map = extra.extruders.into_iter().next().unwrap_or_default();
            map.insert("extruder_nr".to_string(), index.to_string());
            extruders.push(map);
        }
        resolved.extruders = extruders;
        resolved
            .global
            .insert("machine_extruder_count".to_string(), slot_count.to_string());
    }

    // A number typed into the sheet is an override of the profile, whatever key
    // it names. Those values travelled all the way from the panel as `extra` and
    // the only ones that reached the engine were the handful of keys with
    // hand-written handling below, so every other field in the sheet was a
    // facade: it moved, it was remembered, and the slice ignored it. Merged in
    // last, an override wins over the profile, which is what an override means.
    for (key, value) in settings
        .extra
        .iter()
        .chain(models.iter().flat_map(|model| model.settings.iter()))
    {
        if !value.is_empty() {
            resolved.global.insert(key.clone(), value.clone());
        }
    }
    // CuraEngine expects `by object` / `all`; Bambu profiles use one_at_a_time / all_at_once.
    if let Some(seq) = resolved.global.get_mut("print_sequence") {
        let mapped = match seq.as_str() {
            "one_at_a_time" | "by_object" | "by object" => "by object",
            "all_at_once" | "all" => "all",
            other => other,
        };
        if mapped != seq.as_str() {
            *seq = mapped.to_string();
        }
    }
    // The layer height also drives the layer estimate, so an overridden height
    // has to be read back out of the merged map rather than the profile's copy.
    if let Some(height) = resolved
        .global
        .get("layer_height")
        .and_then(|value| value.parse::<f64>().ok())
    {
        if height > 0.0 {
            resolved.summary.layer_height = height;
        }
    }

    // Cura's own UI treats cool_fan_enabled as the switch over the fan and leaves
    // the speeds alone. The engine only reads the speeds, so with the switch off
    // it keeps blowing at cool_fan_speed. Turning cooling off has to mean the fan
    // is off, whichever field it was switched from.
    let cooling_off = settings
        .extra
        .iter()
        .chain(models.iter().flat_map(|model| model.settings.iter()))
        .any(|(key, value)| {
            key == "cool_fan_enabled" && matches!(value.as_str(), "False" | "false")
        });
    if cooling_off {
        for key in [
            "cool_fan_speed",
            "cool_fan_speed_0",
            "cool_fan_speed_min",
            "cool_fan_speed_max",
        ] {
            force_setting(&mut resolved, key, "0");
        }
    }

    let layer_height = resolved.summary.layer_height.max(0.05);
    let nonplanar_active = settings.nonplanar.mode.active();
    if nonplanar_active {
        // Refuse before the engine runs: without a measured head envelope nothing
        // downstream can verify the curved pass, and a silent planar print would
        // hide the request.
        settings
            .nonplanar
            .check_printer(printer_id)
            .map_err(Error::message)?;
        force_setting(&mut resolved, "ironing_enabled", "false");
    }
    let mut undeformed_paths = Vec::new();
    let mut estimated_layers = 1u32;
    let mut baked_paths = Vec::new();
    // The header's bounding box, measured on the same meshes the engine is
    // handed. CuraEngine fills it with `f32::MAX` when it is given bare files.
    let mut scene_min = [f64::INFINITY; 3];
    let mut scene_max = [f64::NEG_INFINITY; 3];
    let model_dir = output_path.with_extension("models");
    fs::create_dir_all(&model_dir)?;
    // Bake every model into MACHINE space first. Height is measured there; the
    // engine then wants bed-centre coordinates, which is a pure XY shift and does
    // not change the height.
    let mut machine_meshes: Vec<TriangleMesh> = Vec::with_capacity(models.len());
    for model in models.iter() {
        let baked = mesh::bake(&model.mesh, &model.transform);
        let (min, max) = baked.bounds();
        let height = (max[2] - min[2]).max(0.0) as f64;
        estimated_layers = estimated_layers.max((height / layer_height).ceil().max(1.0) as u32);
        machine_meshes.push(baked);
    }
    // One field for the whole plate. The reference mesh is the merged MACHINE-space
    // geometry of every model, so the auto spans, the auto placement, the slope
    // limiting and the field the post-pass maps with are all built once, from the
    // coordinates the G-code is written in.
    let mut nonplanar_ctx: Option<shale_nonplanar::NonplanarContext> = None;
    if nonplanar_active {
        let np_meshes: Vec<shale_nonplanar::mesh::Mesh> = machine_meshes
            .iter()
            .map(crate::nonplanar_bridge::to_nonplanar)
            .collect();
        let reference = shale_nonplanar::merge_meshes(&np_meshes);
        // Auto spans come from the plate geometry in MACHINE space, summed over
        // every model, so a bridge on one part can be supported from another.
        let support_spans = if settings.nonplanar.mode == shale_nonplanar::NonplanarMode::Auto {
            machine_meshes
                .iter()
                .flat_map(|mesh| {
                    let analysis = crate::auto_spans::analyse_for_nonplanar(mesh);
                    crate::auto_spans::spans_for_auto(&analysis.spans)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        on_progress(EngineProgress::new(0.05, "Planning curved layers"));
        nonplanar_ctx = Some(shale_nonplanar::plan_field_reporting(
            &reference,
            &settings.nonplanar,
            layer_height,
            &support_spans,
            &mut |fraction, message| {
                on_progress(EngineProgress::new(0.05 + fraction * 0.1, message));
            },
        ));
    }
    for (index, model) in models.iter().enumerate() {
        let machine = &machine_meshes[index];
        let mut baked = machine.clone();
        mesh::to_engine_space(
            &mut baked,
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_center_is_zero,
        );
        if let Some(ctx) = nonplanar_ctx.as_ref() {
            let und_path = model_dir.join(format!("model-{index}-undeformed.stl"));
            mesh::write_binary_stl(&und_path, &baked)?;
            undeformed_paths.push(EngineMesh {
                path: und_path,
                settings: model.settings.clone(),
                extruder: model.slot,
            });
            let np_in = crate::nonplanar_bridge::to_nonplanar(machine);
            let deformed = shale_nonplanar::deform_with_field(&np_in, &ctx.field);
            baked = crate::nonplanar_bridge::from_nonplanar(&deformed);
            mesh::to_engine_space(
                &mut baked,
                resolved.summary.machine_width,
                resolved.summary.machine_depth,
                resolved.summary.machine_center_is_zero,
            );
        }
        let (engine_min, engine_max) = mesh::bounds(&baked);
        for axis in 0..3 {
            scene_min[axis] = scene_min[axis].min(engine_min[axis]);
            scene_max[axis] = scene_max[axis].max(engine_max[axis]);
        }
        let path = model_dir.join(format!("model-{index}.stl"));
        mesh::write_binary_stl(&path, &baked)?;
        baked_paths.push(EngineMesh {
            path,
            settings: model.settings.clone(),
            extruder: model.slot,
        });
    }

    let mut modifiers = Vec::new();
    // Painting means "support here, and only here": the plate's own support is
    // turned off and the columns are the whole of it.
    // Only an enforce stroke turns the profile's own support off: a plate you
    // have blocked a corner of still wants support everywhere else.
    let painted_present = painted
        .iter()
        .any(|patch| !patch.triangles.is_empty() && patch.role == paint::PaintRole::Enforce);
    if painted_present {
        force_setting(&mut resolved, "support_enable", "false");
        force_setting(&mut resolved, "support_meshes_present", "true");
    }
    for patch in painted.iter().filter(|patch| !patch.triangles.is_empty()) {
        let Some(model) = models.get(patch.model) else {
            continue;
        };
        // Straight down has to mean straight down on the bed, so the part is
        // placed first and the prisms are dropped from where it really is. A
        // turned part would otherwise grow columns along its own axis.
        let placed = model.mesh.clone();
        mesh::bake(&placed, &model.transform);
        let mut prisms = paint::prism_mesh(&placed, &patch.triangles, 0.0);
        if prisms.triangles.is_empty() {
            continue;
        }
        mesh::to_engine_space(
            &mut prisms,
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_center_is_zero,
        );
        let role = match patch.role {
            paint::PaintRole::Enforce => supports::RegionRole::Enforce,
            paint::PaintRole::Block => supports::RegionRole::Block,
        };
        let path = model_dir.join(format!("painted-{}-{}.stl", patch.model, role.name()));
        mesh::write_binary_stl(&path, &prisms)?;
        modifiers.push(EngineMesh {
            path,
            settings: supports::modifier_settings(role),
            extruder: None,
        });
    }
    if let Some(plan) = support {
        let enforced = plan
            .regions
            .iter()
            .any(|region| region.enabled && region.role == supports::RegionRole::Enforce);
        if enforced || plan.regions.iter().any(|region| region.enabled) {
            force_setting(&mut resolved, "support_enable", "false");
            force_setting(
                &mut resolved,
                "support_meshes_present",
                if enforced { "true" } else { "false" },
            );
        }
        for region in plan.regions.iter().filter(|region| region.enabled) {
            let mut mesh = supports::region_mesh(region);
            mesh::to_engine_space(
                &mut mesh,
                resolved.summary.machine_width,
                resolved.summary.machine_depth,
                resolved.summary.machine_center_is_zero,
            );
            let path = model_dir.join(format!("{}-{}.stl", region.role.name(), region.id));
            mesh::write_binary_stl(&path, &mesh)?;
            modifiers.push(EngineMesh {
                path,
                settings: supports::modifier_settings(region.role),
                extruder: None,
            });
        }
    }

    // A machine that ships its own start sequence gets it, filled in, instead
    // of Cura's generic Marlin one.
    let machine_gcode = machine_sequence(catalog, &resolved, models)?;
    if let Some((start, end)) = &machine_gcode {
        resolved
            .global
            .insert("machine_start_gcode".into(), start.clone());
        resolved
            .global
            .insert("machine_end_gcode".into(), end.clone());
    }

    let mut planar_baseline_gcode = String::new();
    if nonplanar_active && !undeformed_paths.is_empty() {
        let baseline_path = output_path.with_extension("planar-baseline.gcode");
        let baseline = engine.slice(
            &EngineRequest {
                global: resolved.global.clone(),
                extruders: resolved.extruders.clone(),
                models: undeformed_paths,
                modifiers: modifiers.clone(),
                output_gcode: baseline_path.clone(),
                estimated_layers,
            },
            on_progress,
        )?;
        planar_baseline_gcode = fs::read_to_string(&baseline.gcode_path)?;
        let _ = fs::remove_file(&baseline.gcode_path);
    }

    let output = engine.slice(
        &EngineRequest {
            global: resolved.global.clone(),
            extruders: resolved.extruders.clone(),
            models: baked_paths,
            modifiers,
            output_gcode: output_path.clone(),
            estimated_layers,
        },
        on_progress,
    )?;
    let mut gcode = fs::read_to_string(&output.gcode_path)?;
    let defer_post_pass = nonplanar_active && settings.nonplanar.defer_post_pass;
    let deferred_engine_gcode = defer_post_pass.then(|| gcode.clone());
    let deferred_planar_baseline = defer_post_pass.then(|| planar_baseline_gcode.clone());
    let deferred_context = defer_post_pass.then(|| nonplanar_ctx.clone()).flatten();
    let mut nonplanar_report = None;
    if let (true, Some(context)) = (nonplanar_active, nonplanar_ctx.as_ref()) {
        if !settings.nonplanar.defer_post_pass {
            let machine_height = resolved.summary.machine_height.max(1.0);
            let max_feed = machine_max_feedrate_mm_per_min(&resolved);
            match shale_nonplanar::apply_post_pass(shale_nonplanar::PostPassArgs {
                deformed_slice_gcode: &gcode,
                planar_baseline_gcode: &planar_baseline_gcode,
                context,
                options: &settings.nonplanar,
                printer_id,
                limits: shale_nonplanar::MachineLimits {
                    z_max_mm: machine_height,
                    max_feed_mm_per_min: max_feed,
                },
            }) {
                Ok((mapped, mut report)) => {
                    report
                        .notes
                        .push("ironing_enabled was forced off for curved-top slicing".into());
                    gcode = mapped;
                    nonplanar_report = Some(report);
                }
                // Strict mode: the caller asked for a guarantee, so a curved file that
                // cannot be verified is an error instead of a silent planar print.
                // The engine has already written its raw file, still carrying the
                // 6666 s / 0 m placeholder. Leaving that at the caller's path looks
                // like a finished print.
                Err(err) => {
                    discard_rejected_output(&output.gcode_path);
                    return Err(Error::message(err));
                }
            }
        }
    }
    // CuraEngine does not always say the print time on stdout; it writes it
    // into the file's own `;TIME:` line. The header is read before the summary
    // is allowed to claim there is no estimate, because for the A1 profile
    // there always was one — in the file.
    let gcode = gcode::annotate_header(&gcode, output.print_time_seconds);
    // Read back rather than guess: `annotate_header` has just put the measured
    // figure, or the last `;TIME_ELAPSED:`, into the `;TIME:` line. Doing this
    // in the other order reported 6666 — the placeholder — as 111 minutes.
    let print_time_seconds = output
        .print_time_seconds
        .or_else(|| gcode::header_facts(&gcode).seconds);
    let flavour = GcodeFlavour::from_cura(
        resolved
            .global
            .get("machine_gcode_flavor")
            .map(String::as_str)
            .unwrap_or(""),
    );
    let gcode = instructions::apply(&gcode, instructions, flavour)?;
    // A machine with its own sequence must not be heated by the engine first.
    let gcode = if machine_gcode.is_some() {
        crate::bambu::strip_engine_prelude(&gcode)
    } else {
        gcode
    };
    let gcode = crate::bambu::rewrite_header_bounds(&gcode, scene_min, scene_max);
    let filament_metres = gcode::filament_length_metres(&gcode);
    fs::write(&output.gcode_path, &gcode)?;
    let toolpath = gcode::parse_toolpaths(&gcode);
    let _ = fs::remove_dir_all(&model_dir);
    Ok(SliceOutcome {
        gcode_path: output.gcode_path,
        resolved,
        toolpath,
        print_time_seconds,
        filament_metres,
        flavour,
        machine_gcode: machine_gcode.is_some(),
        log_tail: output.log_tail,
        nonplanar_report,
        engine_gcode: deferred_engine_gcode,
        planar_baseline_gcode: deferred_planar_baseline,
        nonplanar_context: deferred_context,
    })
}

/// Slice the same job twice: once with the proposed meshes, once with
/// Cura's everywhere supports and no meshes. Volumes stay geometric.
/// Times and support filament come from the engine.
pub fn compare_support(
    catalog: &Catalog,
    engine: &dyn SlicingEngine,
    printer_id: &str,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    settings: UserSettings,
    models: &[PlacedModel],
    plan: &SupportPlan,
    on_progress: &mut dyn FnMut(EngineProgress),
) -> Result<crate::supports::SupportComparison, Error> {
    let dir = std::env::temp_dir().join("shale");
    fs::create_dir_all(&dir)?;
    let mut smart_settings = settings.clone();
    smart_settings.support_enable = Some(false);
    let smart = slice(
        catalog,
        engine,
        printer_id,
        variant_name.clone(),
        quality_type.clone(),
        std::slice::from_ref(&material_id),
        smart_settings,
        models,
        &[],
        Some(plan),
        &[],
        dir.join("support-smart.gcode"),
        on_progress,
    )?;
    let mut everywhere_settings = settings;
    everywhere_settings.support_enable = Some(true);
    let everywhere = slice(
        catalog,
        engine,
        printer_id,
        variant_name,
        quality_type,
        std::slice::from_ref(&material_id),
        everywhere_settings,
        models,
        &[],
        None,
        &[],
        dir.join("support-everywhere.gcode"),
        on_progress,
    )?;
    let smart_gcode = fs::read_to_string(&smart.gcode_path).unwrap_or_default();
    let everywhere_gcode = fs::read_to_string(&everywhere.gcode_path).unwrap_or_default();
    let mut comparison = plan.comparison.clone();
    if let Some(seconds) = smart.print_time_seconds {
        comparison.proposed_time_seconds = seconds;
    }
    if let Some(seconds) = everywhere.print_time_seconds {
        comparison.everywhere_time_seconds = seconds;
    }
    comparison.times_from_engine = true;
    comparison.proposed_support_metres = Some(gcode::support_filament_metres(&smart_gcode));
    comparison.everywhere_support_metres = Some(gcode::support_filament_metres(&everywhere_gcode));
    Ok(comparison)
}

/// Remove the engine's raw file after a strict rejection.
///
/// The post-pass runs after CuraEngine has written `output_path`. On the
/// success path that file is replaced with the annotated G-code. On a strict
/// rejection nothing replaces it, so the placeholder header (`;TIME:6666`,
/// `;Filament used: 0m`, unset bounds) would stay where the caller asked to
/// save a print.
fn discard_rejected_output(output_path: &std::path::Path) {
    let _ = fs::remove_file(output_path);
    let _ = fs::remove_dir_all(output_path.with_extension("models"));
}

fn force_setting(resolved: &mut ResolvedSettings, key: &str, value: &str) {
    resolved.global.insert(key.to_string(), value.to_string());
    for extruder in &mut resolved.extruders {
        extruder.insert(key.to_string(), value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::discard_rejected_output;
    use std::fs;

    #[test]
    fn a_rejected_strict_slice_does_not_leave_the_engine_file() {
        let dir = std::env::temp_dir().join(format!("shale-reject-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let output = dir.join("part.gcode");
        fs::write(&output, b";TIME:6666\n;Filament used: 0m\n").unwrap();
        let models = output.with_extension("models");
        fs::create_dir_all(&models).unwrap();
        fs::write(models.join("part.stl"), b"solid").unwrap();

        discard_rejected_output(&output);

        assert!(
            !output.exists(),
            "the placeholder gcode must not remain at the output path"
        );
        assert!(
            !models.exists(),
            "the baked meshes must not remain beside a rejected slice"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
