//! The job the window and the MCP server both drive.
//!
//! Loading a mesh, resolving settings, slicing, and compiling custom
//! instructions all go through this type. Callers do not grow a second
//! slicer beside it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, MaterialInfo, PrinterInfo, QualityInfo, VariantInfo};
use crate::engine::{EngineProgress, SlicingEngine};
use crate::error::Error;
use crate::gcode;
use crate::instructions::CustomInstruction;
use crate::job::{self, PlacedModel, SliceOutcome};
use crate::mesh::{self, Transform, TriangleMesh};
use crate::resolver::{ResolveRequest, ResolvedSettings, UserSettings};
use crate::supports::{self, SupportParams, SupportPlan};

#[derive(Clone, Debug, Serialize)]
pub struct ModelRecord {
    pub id: u32,
    pub name: String,
    pub transform: Transform,
}

#[derive(Clone, Debug, Serialize)]
pub struct StoredInstruction {
    pub id: u32,
    pub instruction: CustomInstruction,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SettingExplanation {
    pub key: String,
    pub value: String,
    pub label: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobSliceSummary {
    pub layer_count: usize,
    pub print_time_seconds: Option<f64>,
    pub filament_metres: f64,
    pub warning_count: usize,
    pub warnings: Vec<String>,
    pub gcode_path: String,
    pub flavour: String,
}

pub struct Session {
    catalog: Catalog,
    engine: Box<dyn SlicingEngine + Send + Sync>,
    printer_id: Option<String>,
    variant_name: Option<String>,
    quality_type: Option<String>,
    material_id: Option<String>,
    settings: UserSettings,
    models: Vec<Loaded>,
    next_model: u32,
    instructions: Vec<(u32, CustomInstruction)>,
    next_instruction: u32,
    support: Option<SupportPlan>,
    support_model: Option<u32>,
    last: Option<SliceOutcome>,
}

struct Loaded {
    id: u32,
    name: String,
    mesh: TriangleMesh,
    transform: Transform,
    settings: Vec<(String, String)>,
}

impl Session {
    pub fn new(catalog: Catalog, engine: Box<dyn SlicingEngine + Send + Sync>) -> Self {
        let printer_id = catalog
            .printer("creality_ender3")
            .map(|_| "creality_ender3".to_string())
            .or_else(|| catalog.printers().first().map(|printer| printer.id.clone()));
        let (variant_name, quality_type, material_id) = printer_id
            .as_deref()
            .and_then(|id| catalog.printer(id))
            .map(|printer| {
                (
                    printer.preferred_variant_name.clone(),
                    printer.preferred_quality_type.clone(),
                    printer.preferred_material.clone(),
                )
            })
            .unwrap_or((None, None, None));
        Self {
            catalog,
            engine,
            printer_id,
            variant_name,
            quality_type,
            material_id,
            settings: UserSettings::default(),
            models: Vec::new(),
            next_model: 1,
            instructions: Vec::new(),
            next_instruction: 1,
            support: None,
            support_model: None,
            last: None,
        }
    }

    pub fn list_printers(&self) -> Vec<PrinterInfo> {
        self.catalog.printers()
    }

    pub fn list_materials(&self, printer_id: Option<&str>) -> Result<Vec<MaterialInfo>, Error> {
        let id = self.printer(printer_id)?;
        let diameter = self.resolve(Some(id))?.summary.material_diameter;
        Ok(self.catalog.materials_for(diameter))
    }

    pub fn list_profiles(
        &self,
        printer_id: Option<&str>,
    ) -> Result<(Vec<VariantInfo>, Vec<QualityInfo>), Error> {
        let id = self.printer(printer_id)?.to_string();
        Ok((
            self.catalog.variants_for(&id),
            self.catalog.qualities_for(&id),
        ))
    }

    pub fn set_printer(
        &mut self,
        printer_id: &str,
        variant_name: Option<String>,
        quality_type: Option<String>,
        material_id: Option<String>,
    ) -> Result<(), Error> {
        let (preferred_variant, preferred_quality, preferred_material) = {
            let printer = self
                .catalog
                .printer(printer_id)
                .ok_or_else(|| Error::UnknownPrinter(printer_id.to_string()))?;
            (
                printer.preferred_variant_name.clone(),
                printer.preferred_quality_type.clone(),
                printer.preferred_material.clone(),
            )
        };
        self.variant_name = variant_name.or(preferred_variant);
        self.quality_type = quality_type.or(preferred_quality);
        self.material_id = material_id.or(preferred_material);
        self.printer_id = Some(printer_id.to_string());
        Ok(())
    }

    pub fn load_model(&mut self, path: &Path) -> Result<ModelRecord, Error> {
        let mesh = mesh::load_mesh(path)?;
        let transform = self.placement(&mesh)?;
        let id = self.next_model;
        self.next_model += 1;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model")
            .to_string();
        let record = ModelRecord {
            id,
            name: name.clone(),
            transform: transform.clone(),
        };
        self.models.push(Loaded {
            id,
            name,
            mesh,
            transform,
            settings: Vec::new(),
        });
        Ok(record)
    }

    pub fn models(&self) -> Vec<ModelRecord> {
        self.models
            .iter()
            .map(|model| ModelRecord {
                id: model.id,
                name: model.name.clone(),
                transform: model.transform.clone(),
            })
            .collect()
    }

    pub fn transform_model(
        &mut self,
        id: u32,
        translation: Option<[f64; 3]>,
        rotation_deg: Option<[f64; 3]>,
        scale: Option<[f64; 3]>,
    ) -> Result<Transform, Error> {
        let transform = {
            let model = self.model_mut(id)?;
            if let Some(value) = translation {
                model.transform.translation = value;
            }
            if let Some(value) = rotation_deg {
                model.transform.rotation_deg = value;
            }
            if let Some(value) = scale {
                model.transform.scale = value;
            }
            model.transform.clone()
        };
        self.invalidate_support(id);
        Ok(transform)
    }

    pub fn arrange_model(&mut self, id: u32) -> Result<Transform, Error> {
        let mesh = self.model(id)?.mesh.clone();
        let transform = self.placement(&mesh)?;
        self.model_mut(id)?.transform = transform.clone();
        self.invalidate_support(id);
        Ok(transform)
    }

    pub fn drop_model(&mut self, id: u32) -> Result<Transform, Error> {
        let transform = {
            let model = self.model_mut(id)?;
            mesh::drop_to_bed(&model.mesh, &mut model.transform);
            model.transform.clone()
        };
        self.invalidate_support(id);
        Ok(transform)
    }

    pub fn get_settings(&self, keys: &[String]) -> Result<SettingsView, Error> {
        let resolved = self.resolve(None)?;
        let printer = self.printer_id.clone().unwrap_or_default();
        let wanted: Vec<String> = if keys.is_empty() {
            DEFAULT_KEYS.iter().map(|key| (*key).to_string()).collect()
        } else {
            keys.to_vec()
        };
        let settings = wanted
            .into_iter()
            .filter_map(|key| {
                let value = resolved.global.get(&key).cloned().or_else(|| {
                    resolved
                        .extruders
                        .first()
                        .and_then(|extruder| extruder.get(&key).cloned())
                })?;
                let (label, description) = self.catalog.setting_text(&printer, &key);
                Some(SettingExplanation {
                    key,
                    value,
                    label,
                    description,
                })
            })
            .collect();
        Ok(SettingsView {
            printer_id: printer,
            variant_name: self.variant_name.clone(),
            quality_type: self.quality_type.clone(),
            material_id: resolved.material_id.clone(),
            summary: resolved.summary.clone(),
            warning_count: resolved.warning_count,
            settings,
        })
    }

    pub fn set_settings(&mut self, patch: SettingsPatch) -> Result<(), Error> {
        if let Some(id) = patch.printer_id {
            self.set_printer(
                &id,
                patch.variant_name.clone(),
                patch.quality_type.clone(),
                patch.material_id.clone(),
            )?;
        } else {
            if patch.variant_name.is_some() {
                self.variant_name = patch.variant_name;
            }
            if patch.quality_type.is_some() {
                self.quality_type = patch.quality_type;
            }
            if patch.material_id.is_some() {
                self.material_id = patch.material_id;
            }
        }
        if let Some(value) = patch.layer_height {
            self.settings.layer_height = Some(value);
        }
        if let Some(value) = patch.infill_sparse_density {
            self.settings.infill_sparse_density = Some(value);
        }
        if let Some(value) = patch.support_enable {
            self.settings.support_enable = Some(value);
        }
        if let Some(value) = patch.material_print_temperature {
            self.settings.material_print_temperature = Some(value);
        }
        if let Some(value) = patch.material_bed_temperature {
            self.settings.material_bed_temperature = Some(value);
        }
        Ok(())
    }

    pub fn instructions(&self) -> Vec<StoredInstruction> {
        self.instructions
            .iter()
            .map(|(id, instruction)| StoredInstruction {
                id: *id,
                summary: instruction.summary(),
                instruction: instruction.clone(),
            })
            .collect()
    }

    pub fn add_instruction(&mut self, instruction: CustomInstruction) -> StoredInstruction {
        let id = self.next_instruction;
        self.next_instruction += 1;
        let stored = StoredInstruction {
            id,
            summary: instruction.summary(),
            instruction: instruction.clone(),
        };
        self.instructions.push((id, instruction));
        stored
    }

    pub fn remove_instruction(&mut self, id: u32) -> Result<(), Error> {
        let before = self.instructions.len();
        self.instructions.retain(|(existing, _)| *existing != id);
        if self.instructions.len() == before {
            return Err(Error::message(format!("no instruction with id {id}")));
        }
        Ok(())
    }

    pub fn analyse_overhangs(
        &self,
        model_id: u32,
        params: SupportParams,
    ) -> Result<supports::Analysis, Error> {
        let model = self.model(model_id)?;
        let baked = mesh::bake(&model.mesh, &model.transform);
        Ok(supports::analyse(&baked, &params))
    }

    pub fn propose_supports(
        &mut self,
        model_id: u32,
        params: SupportParams,
    ) -> Result<SupportPlan, Error> {
        let baked = {
            let model = self.model(model_id)?;
            mesh::bake(&model.mesh, &model.transform)
        };
        let plan = supports::propose(&baked, &params);
        self.support = Some(plan.clone());
        self.support_model = Some(model_id);
        Ok(plan)
    }

    pub fn support_plan(&self) -> Option<&SupportPlan> {
        self.support.as_ref()
    }

    /// `enabled` lists the region ids to keep. `None` keeps every region.
    pub fn apply_supports(&mut self, enabled: Option<&[u32]>) -> Result<SupportPlan, Error> {
        let plan = self
            .support
            .as_mut()
            .ok_or_else(|| Error::message("propose supports before applying them"))?;
        supports::set_enabled(plan, enabled);
        Ok(plan.clone())
    }

    pub fn slice(
        &mut self,
        on_progress: &mut dyn FnMut(EngineProgress),
    ) -> Result<JobSliceSummary, Error> {
        let printer_id = self
            .printer_id
            .clone()
            .ok_or_else(|| Error::message("choose a printer before slicing"))?;
        let placed: Vec<PlacedModel> = self
            .models
            .iter()
            .map(|model| PlacedModel {
                slot: None,
                name: model.name.clone(),
                mesh: model.mesh.clone(),
                transform: model.transform.clone(),
                settings: model.settings.clone(),
            })
            .collect();
        let instructions: Vec<CustomInstruction> = self
            .instructions
            .iter()
            .map(|(_, instruction)| instruction.clone())
            .collect();
        let output = std::env::temp_dir()
            .join("shale")
            .join(format!("mcp-{}.gcode", std::process::id()));
        let outcome = job::slice(
            &self.catalog,
            self.engine.as_ref(),
            &printer_id,
            self.variant_name.clone(),
            self.quality_type.clone(),
            std::slice::from_ref(&self.material_id),
            self.settings.clone(),
            &placed,
            &instructions,
            self.support.as_ref(),
            &[],
            output,
            on_progress,
        )?;
        let summary = summary_of(&outcome);
        self.last = Some(outcome);
        Ok(summary)
    }

    pub fn summary(&self) -> Option<JobSliceSummary> {
        self.last.as_ref().map(summary_of)
    }

    pub fn export_gcode(&self, path: &Path) -> Result<PathBuf, Error> {
        let Some(last) = &self.last else {
            return Err(Error::message("slice a model before exporting G-code"));
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&last.gcode_path, path)?;
        Ok(path.to_path_buf())
    }

    pub fn gcode(&self) -> Result<String, Error> {
        let Some(last) = &self.last else {
            return Err(Error::message("nothing has been sliced yet"));
        };
        Ok(std::fs::read_to_string(&last.gcode_path)?)
    }

    fn invalidate_support(&mut self, id: u32) {
        if self.support_model == Some(id) {
            self.support = None;
            self.support_model = None;
        }
    }

    fn placement(&self, mesh: &TriangleMesh) -> Result<Transform, Error> {
        let resolved = self.resolve(None)?;
        Ok(mesh::place_on_bed(
            mesh,
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_center_is_zero,
        ))
    }

    fn resolve(&self, printer_override: Option<&str>) -> Result<ResolvedSettings, Error> {
        let printer_id = printer_override
            .map(str::to_string)
            .or_else(|| self.printer_id.clone())
            .ok_or_else(|| Error::message("choose a printer first"))?;
        self.catalog.resolve(&ResolveRequest {
            printer_id,
            variant_name: self.variant_name.clone(),
            quality_type: self.quality_type.clone(),
            material_id: self.material_id.clone(),
            intent_category: None,
            settings: self.settings.clone(),
        })
    }

    fn printer<'a>(&'a self, override_id: Option<&'a str>) -> Result<&'a str, Error> {
        override_id
            .or(self.printer_id.as_deref())
            .ok_or_else(|| Error::message("choose a printer first"))
    }

    fn model(&self, id: u32) -> Result<&Loaded, Error> {
        self.models
            .iter()
            .find(|model| model.id == id)
            .ok_or_else(|| Error::message(format!("no model with id {id}")))
    }

    fn model_mut(&mut self, id: u32) -> Result<&mut Loaded, Error> {
        self.models
            .iter_mut()
            .find(|model| model.id == id)
            .ok_or_else(|| Error::message(format!("no model with id {id}")))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SettingsView {
    pub printer_id: String,
    pub variant_name: Option<String>,
    pub quality_type: Option<String>,
    pub material_id: String,
    pub summary: crate::resolver::ProfileSummary,
    pub warning_count: usize,
    pub settings: Vec<SettingExplanation>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SettingsPatch {
    pub printer_id: Option<String>,
    pub variant_name: Option<String>,
    pub quality_type: Option<String>,
    pub material_id: Option<String>,
    pub layer_height: Option<f64>,
    pub infill_sparse_density: Option<f64>,
    pub support_enable: Option<bool>,
    pub material_print_temperature: Option<f64>,
    pub material_bed_temperature: Option<f64>,
}

const DEFAULT_KEYS: &[&str] = &[
    "layer_height",
    "layer_height_0",
    "infill_sparse_density",
    "infill_line_distance",
    "support_enable",
    "material_print_temperature",
    "material_bed_temperature",
    "line_width",
    "machine_nozzle_size",
    "machine_width",
    "machine_depth",
    "machine_height",
    "adhesion_type",
    "speed_print",
    "machine_gcode_flavor",
];

fn summary_of(outcome: &SliceOutcome) -> JobSliceSummary {
    let filament = if outcome.filament_metres > 0.0 {
        outcome.filament_metres
    } else {
        gcode::filament_length_metres(
            &std::fs::read_to_string(&outcome.gcode_path).unwrap_or_default(),
        )
    };
    JobSliceSummary {
        layer_count: outcome.toolpath.layers.len(),
        print_time_seconds: outcome.print_time_seconds,
        filament_metres: filament,
        warning_count: outcome.resolved.warning_count,
        warnings: outcome.resolved.warnings.clone(),
        gcode_path: outcome.gcode_path.display().to_string(),
        flavour: outcome.flavour.name().to_string(),
    }
}
