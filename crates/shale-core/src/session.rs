//! The job the window and the MCP server both drive.
//!
//! Loading a mesh, resolving settings, slicing, and compiling custom
//! instructions all go through this type. Callers do not grow a second
//! slicer beside it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, MaterialInfo, PrinterInfo, QualityInfo, VariantInfo};
use crate::engine::{EngineProgress, SlicingEngine};
use crate::error::Error;
use crate::gcode;
use crate::instructions::CustomInstruction;
use crate::job::{self, PlacedModel, SliceOutcome};
use crate::mesh::{self, BuildObject, PlateGroup, Transform, TriangleMesh};
use crate::project_settings::{self, ProjectSettings, ProjectSettingsImport};
use crate::resolver::{ResolveRequest, ResolvedSettings, UserSettings};
use crate::supports::{self, SupportParams, SupportPlan};

static SLICE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize)]
pub struct ModelRecord {
    pub id: u32,
    pub name: String,
    pub transform: Transform,
}

/// One plate a loaded 3MF can be sliced as.
#[derive(Clone, Debug, Serialize)]
pub struct SessionPlate {
    /// 1-based plate number from the file.
    pub number: u32,
    pub name: String,
    pub objects: usize,
}

/// What `load_model` put on the bed, plus the plates and project settings of the file.
#[derive(Clone, Debug, Serialize)]
pub struct LoadOutcome {
    pub id: u32,
    pub name: String,
    pub transform: Transform,
    pub models: Vec<ModelRecord>,
    pub plates: Vec<SessionPlate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded: Option<ProjectSettingsImport>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonplanar_safety_ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonplanar_planar_equivalence_ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonplanar_collision_fixes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonplanar_report: Option<shale_nonplanar::NonplanarReport>,
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
    source_path: Option<PathBuf>,
    source_plates: Vec<PlateGroup>,
    listed_plates: Vec<SessionPlate>,
    project_settings: Option<ProjectSettings>,
    ignore_project_settings: bool,
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
        let printer_id = catalog.default_printer_id().or_else(|| {
            catalog
                .printer("demo_ender")
                .map(|_| "demo_ender".to_string())
                .or_else(|| catalog.printers().first().map(|printer| printer.id.clone()))
        });
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
            source_path: None,
            source_plates: Vec::new(),
            listed_plates: Vec::new(),
            project_settings: None,
            ignore_project_settings: false,
        }
    }

    pub fn list_printers(&self) -> Vec<PrinterInfo> {
        self.catalog.selectable_printers()
    }

    pub fn list_materials(&self, printer_id: Option<&str>) -> Result<Vec<MaterialInfo>, Error> {
        let id = self.printer(printer_id)?;
        let diameter = self.resolve(Some(id))?.summary.material_diameter;
        if self.catalog.external.contains(id) {
            return Ok(self.catalog.materials_for_printer(id, diameter));
        }
        Ok(self.catalog.materials_for(diameter))
    }

    pub fn list_profiles(
        &self,
        printer_id: Option<&str>,
    ) -> Result<(Vec<VariantInfo>, Vec<QualityInfo>), Error> {
        let id = self.printer(printer_id)?.to_string();
        Ok((
            self.catalog.variants_for(&id),
            self.catalog.qualities_for(&id, None),
        ))
    }

    pub fn set_printer(
        &mut self,
        printer_id: &str,
        variant_name: Option<String>,
        quality_type: Option<String>,
        material_id: Option<String>,
    ) -> Result<(), Error> {
        if !self
            .catalog
            .selectable_printers()
            .iter()
            .any(|printer| printer.id == printer_id)
        {
            return Err(Error::message(format!(
                "printer '{printer_id}' is not available in this release (Bambu Lab only)"
            )));
        }
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
        let outcome = self.load_on_plate(path, None)?;
        Ok(ModelRecord {
            id: outcome.id,
            name: outcome.name,
            transform: outcome.transform,
        })
    }

    /// Load one plate of a model. `plate` is the 1-based number from the file;
    /// `None` is plate 1. A 3MF replaces whatever was loaded. An STL is added.
    pub fn load_on_plate(&mut self, path: &Path, plate: Option<u32>) -> Result<LoadOutcome, Error> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension == "3mf" {
            let bytes = std::fs::read(path)?;
            let objects = mesh::load_build_objects(&bytes)?;
            let groups = mesh::assign_plates(objects, mesh::read_plate_groups(&bytes));
            if groups.is_empty() {
                return Err(Error::message("that file held no geometry"));
            }
            let number = plate.unwrap_or(1);
            let chosen = groups
                .iter()
                .find(|(group, _)| group.number == number)
                .or_else(|| groups.get(number.saturating_sub(1) as usize))
                .ok_or_else(|| {
                    Error::message(format!(
                        "this file has no plate {number} (it has {})",
                        groups.len()
                    ))
                })?;
            self.models.clear();
            self.support = None;
            self.support_model = None;
            self.last = None;
            let records = self.push_plate(&chosen.1)?;
            self.source_path = Some(path.to_path_buf());
            self.listed_plates = self.plate_list(&groups);
            self.source_plates = groups.iter().map(|(group, _)| group.clone()).collect();
            self.project_settings = project_settings::read_project_settings(&bytes);
            let first = records.first().cloned().ok_or_else(|| {
                Error::message("that plate held no geometry")
            })?;
            return Ok(LoadOutcome {
                id: first.id,
                name: first.name,
                transform: first.transform,
                models: records,
                plates: self.listed_plates.clone(),
                embedded: self.project_settings.as_ref().map(|project| project.summary.clone()),
            });
        }
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
        self.source_path = Some(path.to_path_buf());
        self.source_plates.clear();
        self.project_settings = None;
        self.listed_plates = vec![SessionPlate {
            number: 1,
            name: String::new(),
            objects: self.models.len(),
        }];
        Ok(LoadOutcome {
            id: record.id,
            name: record.name.clone(),
            transform: record.transform.clone(),
            models: vec![record],
            plates: self.listed_plates.clone(),
            embedded: None,
        })
    }

    pub fn set_ignore_project_settings(&mut self, ignore: bool) {
        self.ignore_project_settings = ignore;
    }

    pub fn project_import(&self) -> Option<&ProjectSettingsImport> {
        self.project_settings.as_ref().map(|project| &project.summary)
    }

    fn plate_list(&self, groups: &[(PlateGroup, Vec<BuildObject>)]) -> Vec<SessionPlate> {
        groups
            .iter()
            .map(|(group, objects)| SessionPlate {
                number: group.number,
                name: group.name.clone(),
                objects: objects.len(),
            })
            .collect()
    }

    fn push_plate(&mut self, objects: &[BuildObject]) -> Result<Vec<ModelRecord>, Error> {
        let meshes: Vec<TriangleMesh> = objects.iter().map(|object| object.mesh.clone()).collect();
        let transform = if meshes.len() == 1 {
            self.placement(&meshes[0])?
        } else {
            let resolved = self.resolve(None)?;
            mesh::place_assembly(
                &meshes,
                resolved.summary.machine_width,
                resolved.summary.machine_depth,
                resolved.summary.machine_center_is_zero,
            )
        };
        let mut records = Vec::new();
        for object in objects {
            let id = self.next_model;
            self.next_model += 1;
            let name = if object.name.trim().is_empty() {
                format!("object {}", object.object_id)
            } else {
                object.name.clone()
            };
            let record = ModelRecord {
                id,
                name: name.clone(),
                transform: transform.clone(),
            };
            self.models.push(Loaded {
                id,
                name,
                mesh: object.mesh.clone(),
                transform: transform.clone(),
                settings: Vec::new(),
            });
            records.push(record);
        }
        Ok(records)
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

    /// Centre the part and drop it onto the bed. Rotation and scale stay as they are.
    pub fn arrange_model(&mut self, id: u32) -> Result<Transform, Error> {
        let mesh = self.model(id)?.mesh.clone();
        let pose = self.model(id)?.transform.clone();
        let resolved = self.resolve(None)?;
        let transform = mesh::centre_keeping_pose(
            &mesh,
            &pose,
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_center_is_zero,
        );
        self.model_mut(id)?.transform = transform.clone();
        self.invalidate_support(id);
        Ok(transform)
    }

    /// Plates from the last load. `number` is the same 1-based plate `load_on_plate` and `slice` take.
    pub fn list_plates(&self) -> Vec<SessionPlate> {
        if !self.listed_plates.is_empty() {
            return self.listed_plates.clone();
        }
        if self.models.is_empty() {
            return Vec::new();
        }
        vec![SessionPlate {
            number: 1,
            name: String::new(),
            objects: self.models.len(),
        }]
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
        if let Some(mode) = patch.nonplanar_mode {
            self.settings.nonplanar.mode = shale_nonplanar::NonplanarMode::from_cli_name(&mode)
                .unwrap_or(shale_nonplanar::NonplanarMode::Off);
        }
        if let Some(angle) = patch.nonplanar_max_angle_deg {
            self.settings.nonplanar.max_angle_deg = angle;
        }
        if let Some(strict) = patch.nonplanar_strict {
            self.settings.nonplanar.strict = strict;
        }
        if let Some(kind) = patch.adhesion_type {
            let kind = kind.trim().to_ascii_lowercase();
            if !matches!(kind.as_str(), "none" | "skirt" | "brim") {
                return Err(Error::message(
                    "adhesion must be none, skirt, or brim".to_string(),
                ));
            }
            self.write_setting("adhesion_type", kind)?;
        }
        if let Some(width) = patch.brim_width {
            if !(0.0..=50.0).contains(&width) {
                return Err(Error::message(
                    "brim width must be between 0 and 50 mm".to_string(),
                ));
            }
            self.write_setting("brim_width", format!("{width}"))?;
        }
        if let Some(structure) = patch.support_structure {
            let structure = structure.trim().to_ascii_lowercase();
            match structure.as_str() {
                "normal" | "tree" => self.write_setting("support_structure", structure)?,
                "auto" => {
                    return Err(Error::message(
                        "support structure 'auto' is not available on this engine yet. Use normal or tree."
                            .to_string(),
                    ));
                }
                _ => {
                    return Err(Error::message(
                        "support_structure must be normal, tree, or auto".to_string(),
                    ));
                }
            }
        }
        for (key, value) in patch.settings {
            self.write_setting(&key, value)?;
        }
        Ok(())
    }

    fn write_setting(&mut self, key: &str, value: String) -> Result<(), Error> {
        if !self.catalog.has_setting(key) {
            let message = match self.catalog.nearest_setting(key) {
                Some(name) => format!("unknown setting '{key}'. Did you mean '{name}'?"),
                None => format!("unknown setting '{key}'"),
            };
            return Err(Error::message(message));
        }
        self.settings.extra.retain(|(existing, _)| existing != key);
        self.settings.extra.push((key.to_string(), value));
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

    /// Detect features and plan non-planar surface bands without slicing. Returns
    /// preview geometry for UI overlays plus an explainable placement report.
    pub fn plan_nonplanar_auto(
        &self,
        model_id: u32,
    ) -> Result<shale_nonplanar::AutoPlacementReport, Error> {
        let model = self.model(model_id)?;
        let baked = mesh::bake(&model.mesh, &model.transform);
        let np = crate::nonplanar_bridge::to_nonplanar(&baked);
        let layer_height = self.settings.layer_height.unwrap_or(0.2);
        let options = self.settings.nonplanar.clone();
        let analysis = crate::auto_spans::analyse_for_nonplanar(&baked);
        let spans = crate::auto_spans::spans_for_auto(&analysis.spans);
        Ok(shale_nonplanar::plan_auto_placement(
            &np,
            &spans,
            &options,
            layer_height,
        ))
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
        let stamp = SLICE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let output = std::env::temp_dir().join("shale").join(format!(
            "mcp-{}-{}.gcode",
            std::process::id(),
            stamp
        ));
        let mut settings = self.settings.clone();
        if !self.ignore_project_settings {
            if let Some(project) = &self.project_settings {
                project_settings::apply_project_settings(&mut settings, project);
            }
        }
        let outcome = job::slice(
            &self.catalog,
            self.engine.as_ref(),
            &printer_id,
            self.variant_name.clone(),
            self.quality_type.clone(),
            std::slice::from_ref(&self.material_id),
            settings,
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

    /// Slice every plate of the loaded 3MF. A file with one plate slices once.
    pub fn slice_all_plates(
        &mut self,
        on_progress: &mut dyn FnMut(EngineProgress),
    ) -> Result<Vec<JobSliceSummary>, Error> {
        if self.source_plates.len() <= 1 {
            return Ok(vec![self.slice(on_progress)?]);
        }
        let path = self
            .source_path
            .clone()
            .ok_or_else(|| Error::message("load a model before slicing"))?;
        let numbers: Vec<u32> = self.source_plates.iter().map(|plate| plate.number).collect();
        let mut summaries = Vec::with_capacity(numbers.len());
        for number in numbers {
            self.load_on_plate(&path, Some(number))?;
            summaries.push(self.slice(on_progress)?);
        }
        Ok(summaries)
    }

    /// Replace the loaded models with another plate of the same file.
    pub fn reload_plate(&mut self, plate: u32) -> Result<LoadOutcome, Error> {
        let path = self
            .source_path
            .clone()
            .ok_or_else(|| Error::message("load a model before choosing a plate"))?;
        self.load_on_plate(&path, Some(plate))
    }

    /// Suggestions for the models on the bed. Rules are pure and do not slice.
    pub fn suggestions(&self, query: crate::SuggestQuery) -> Result<Vec<crate::Suggestion>, Error> {
        let resolved = self.resolve(None)?;
        let summary = &resolved.summary;
        let mut settings = std::collections::BTreeMap::new();
        settings.insert("adhesion_type".into(), summary.adhesion_type.clone());
        settings.insert("line_width".into(), format!("{:.3}", summary.line_width));
        settings.insert("speed_print".into(), format!("{:.0}", summary.speed_print));
        settings.insert(
            "support_enable".into(),
            if summary.support_enable { "1" } else { "0" }.into(),
        );
        settings.insert(
            "material_bed_temperature".into(),
            format!("{:.0}", summary.bed_temperature),
        );
        if let Some(enabled) = self.settings.support_enable {
            settings.insert("support_enable".into(), if enabled { "1" } else { "0" }.into());
        }
        for (key, value) in &self.settings.extra {
            settings.insert(key.clone(), value.clone());
        }
        let mut slots: Vec<crate::FilamentSlot> = query
            .slots
            .iter()
            .cloned()
            .map(crate::SlotQuery::into_slot)
            .collect();
        if slots.is_empty() {
            let label = self.material_id.clone().unwrap_or_else(|| "PLA".into());
            slots.push(crate::FilamentSlot {
                index: 0,
                material_id: label.clone(),
                filament_type: label,
                colour: String::new(),
                loaded: true,
            });
        }
        let gcode = self
            .last
            .as_ref()
            .and_then(|outcome| std::fs::read_to_string(&outcome.gcode_path).ok())
            .map(|text| crate::gcode_facts(&text));
        let objects = self
            .models
            .iter()
            .map(|model| crate::SceneObject {
                id: model.id.to_string(),
                name: model.name.clone(),
                mesh: model.mesh.clone(),
                transform: model.transform.clone(),
                plate: 0,
                settings: model.settings.iter().cloned().collect(),
                slot: None,
            })
            .collect();
        let scene = crate::Scene {
            objects,
            printer: crate::PrinterFacts {
                id: resolved.printer_id.clone(),
                name: summary.printer_name.clone(),
                bed_width_mm: summary.machine_width,
                bed_depth_mm: summary.machine_depth,
                bed_height_mm: summary.machine_height,
                nozzle_mm: summary.nozzle_size,
                line_width_mm: summary.line_width,
                speed_mm_s: summary.speed_print,
                bed_temperature_c: summary.bed_temperature,
                enclosed: crate::printer_is_enclosed(&resolved.printer_id),
                multi_material: query.multi_material.unwrap_or(slots.len() > 1),
            },
            slots,
            device_slots: query
                .device_slots
                .map(|slots| slots.into_iter().map(crate::SlotQuery::into_slot).collect()),
            humidity_percent: query.humidity_percent,
            hours_since_dry: query.hours_since_dry,
            settings,
            gcode,
            nonplanar_mode: query
                .nonplanar_mode
                .unwrap_or_else(|| self.settings.nonplanar.mode.name().to_string()),
            tags: query.tags,
            dismissed: query.dismissed,
            global_dismissed: query.global_dismissed,
            shown_once: query.shown_once,
        };
        let found = crate::suggest::evaluate(&scene);
        Ok(match query.limit {
            Some(limit) => found.into_iter().take(limit).collect(),
            None => found,
        })
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
        let id = override_id
            .or(self.printer_id.as_deref())
            .ok_or_else(|| Error::message("choose a printer first"))?;
        // An explicit id that is not a definition used to come back as an empty
        // profile list, which an agent reads as "this printer has no profiles".
        if self.catalog.printer(id).is_none() {
            return Err(Error::UnknownPrinter(id.to_string()));
        }
        Ok(id)
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
    pub nonplanar_mode: Option<String>,
    pub nonplanar_max_angle_deg: Option<f64>,
    /// Turn an unverifiable curved slice into an error instead of a planar fallback.
    pub nonplanar_strict: Option<bool>,
    /// `none`, `skirt`, or `brim`. Written as Cura's `adhesion_type`.
    pub adhesion_type: Option<String>,
    /// Brim width in millimetres. Used with `adhesion_type: brim`.
    pub brim_width: Option<f64>,
    /// `normal` or `tree`. `auto` is reserved and refused until the engine has it.
    pub support_structure: Option<String>,
    /// Any other Cura key, the same as the CLI `--set KEY=VALUE`. Unknown keys
    /// are an error that names a close match when there is one.
    #[serde(default)]
    pub settings: std::collections::BTreeMap<String, String>,
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
    let nonplanar = outcome.nonplanar_report.as_ref();
    JobSliceSummary {
        layer_count: outcome.toolpath.layers.len(),
        print_time_seconds: outcome.print_time_seconds,
        filament_metres: filament,
        warning_count: outcome.resolved.warning_count,
        warnings: outcome.resolved.warnings.clone(),
        gcode_path: outcome.gcode_path.display().to_string(),
        flavour: outcome.flavour.name().to_string(),
        nonplanar_safety_ok: nonplanar.and_then(|r| r.safety.as_ref().map(|s| s.ok)),
        nonplanar_planar_equivalence_ok: nonplanar.map(|r| r.planar_equivalence_ok),
        nonplanar_collision_fixes: nonplanar.map(|r| r.collision_adjustments.len()),
        nonplanar_report: outcome.nonplanar_report.clone(),
    }
}
