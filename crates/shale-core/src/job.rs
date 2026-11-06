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
    let painted_present = painted
        .iter()
        .any(|patch| !patch.triangles.is_empty());
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
        let mut placed = model.mesh.clone();
        mesh::bake(&mut placed, &model.transform);
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
        let path = model_dir.join(format!("painted-{}.stl", patch.model));
        mesh::write_binary_stl(&path, &prisms)?;
        modifiers.push(EngineMesh {
            path,
            settings: supports::modifier_settings(supports::RegionRole::Enforce),
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
        &[material_id.clone()],
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
        &[material_id.clone()],
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
