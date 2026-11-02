//! A slice job: resolve the profile, bake placed meshes to STL, run the engine,
//! then read the toolpath preview back out of the G-code.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::catalog::Catalog;
use crate::engine::{EngineProgress, EngineRequest, SlicingEngine};
use crate::error::Error;
use crate::gcode::{self, Toolpath};
use crate::mesh::{self, Transform, TriangleMesh};
use crate::resolver::{ResolveRequest, ResolvedSettings, UserSettings};

#[derive(Clone, Debug)]
pub struct PlacedModel {
    pub name: String,
    pub mesh: TriangleMesh,
    pub transform: Transform,
}

#[derive(Clone, Debug, Serialize)]
pub struct SliceOutcome {
    pub gcode_path: PathBuf,
    pub resolved: ResolvedSettings,
    pub toolpath: Toolpath,
    pub print_time_seconds: Option<f64>,
    pub log_tail: String,
}

pub fn slice(
    catalog: &Catalog,
    engine: &dyn SlicingEngine,
    printer_id: &str,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    settings: UserSettings,
    models: &[PlacedModel],
    output_path: PathBuf,
    on_progress: &mut dyn FnMut(EngineProgress),
) -> Result<SliceOutcome, Error> {
    if models.is_empty() {
        return Err(Error::message("import a model before slicing"));
    }
    on_progress(EngineProgress {
        fraction: 0.02,
        message: "Resolving printer settings".into(),
    });
    let resolved = catalog.resolve(&ResolveRequest {
        printer_id: printer_id.to_string(),
        variant_name,
        quality_type,
        material_id,
        intent_category: None,
        settings,
    })?;

    let layer_height = resolved.summary.layer_height.max(0.05);
    let mut estimated_layers = 1u32;
    let mut baked_paths = Vec::new();
    let model_dir = output_path.with_extension("models");
    fs::create_dir_all(&model_dir)?;
    for (index, model) in models.iter().enumerate() {
        let baked = mesh::bake(&model.mesh, &model.transform);
        let (_, max) = baked.bounds();
        let (min, _) = baked.bounds();
        let height = (max[2] - min[2]).max(0.0) as f64;
        estimated_layers = estimated_layers.max((height / layer_height).ceil().max(1.0) as u32);
        let path = model_dir.join(format!("model-{index}.stl"));
        mesh::write_binary_stl(&path, &baked)?;
        baked_paths.push(path);
    }

    let output = engine.slice(
        &EngineRequest {
            global: resolved.global.clone(),
            extruders: resolved.extruders.clone(),
            models: baked_paths,
            output_gcode: output_path.clone(),
            estimated_layers,
        },
        on_progress,
    )?;
    let gcode = fs::read_to_string(&output.gcode_path)?;
    let gcode = gcode::annotate_header(&gcode, output.print_time_seconds);
    fs::write(&output.gcode_path, &gcode)?;
    let toolpath = gcode::parse_toolpaths(&gcode);
    let _ = fs::remove_dir_all(&model_dir);
    Ok(SliceOutcome {
        gcode_path: output.gcode_path,
        resolved,
        toolpath,
        print_time_seconds: output.print_time_seconds,
        log_tail: output.log_tail,
    })
}
