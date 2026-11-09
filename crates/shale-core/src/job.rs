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
use crate::resolver::{ResolveRequest, ResolvedSettings, UserSettings};
use crate::paint;
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
            list = global.map(|text| vec![template::value_of(text)]).unwrap_or_default();
        }
        // Left unset when nothing resolved: the render then names the missing
        // name and the slice stops, rather than starting a machine at 0 degrees.
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
        .any(|(key, value)| key == "cool_fan_enabled" && matches!(value.as_str(), "False" | "false"));
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
    let mut estimated_layers = 1u32;
    let mut baked_paths = Vec::new();
    // The header's bounding box, measured on the same meshes the engine is
    // handed. CuraEngine fills it with `f32::MAX` when it is given bare files.
    let mut scene_min = [f64::INFINITY; 3];
    let mut scene_max = [f64::NEG_INFINITY; 3];
    let model_dir = output_path.with_extension("models");
    fs::create_dir_all(&model_dir)?;
    for (index, model) in models.iter().enumerate() {
        let mut baked = mesh::bake(&model.mesh, &model.transform);
        let (_, max) = baked.bounds();
        let (min, _) = baked.bounds();
        let height = (max[2] - min[2]).max(0.0) as f64;
        estimated_layers = estimated_layers.max((height / layer_height).ceil().max(1.0) as u32);
        // Height is measured in machine space. The engine then wants bed-centre
        // coordinates, which is a pure XY shift and does not change the height.
        mesh::to_engine_space(
            &mut baked,
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_center_is_zero,
        );
        let (engine_min, engine_max) = mesh::bounds(&baked);
        for axis in 0..3 {
            scene_min[axis] = scene_min[axis].min(engine_min[axis] as f64);
            scene_max[axis] = scene_max[axis].max(engine_max[axis] as f64);
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
    let gcode = fs::read_to_string(&output.gcode_path)?;
    let gcode = gcode::annotate_header(&gcode, output.print_time_seconds);
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
        print_time_seconds: output.print_time_seconds,
        filament_metres,
        flavour,
        machine_gcode: machine_gcode.is_some(),
        log_tail: output.log_tail,
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

fn force_setting(resolved: &mut ResolvedSettings, key: &str, value: &str) {
    resolved.global.insert(key.to_string(), value.to_string());
    for extruder in &mut resolved.extruders {
        extruder.insert(key.to_string(), value.to_string());
    }
}
