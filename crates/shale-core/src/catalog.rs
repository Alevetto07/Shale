//! Loads the Cura resource tree and the fdm_materials library into memory.
//!
//! Definitions are small enough to keep resident. Instance containers and
//! materials are indexed once so picking a printer does not re-read disk.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::definition::{self, Definition, RawSetting};
use crate::error::Error;
use crate::instance::{self, InstanceFile};
use crate::material::{self, Material};

#[derive(Clone, Debug)]
pub struct ResourcePaths {
    pub cura: PathBuf,
    pub materials: PathBuf,
}

impl ResourcePaths {
    pub fn resources(&self) -> PathBuf {
        let nested = self.cura.join("resources");
        if nested.is_dir() {
            nested
        } else {
            self.cura.clone()
        }
    }

    /// Walk upwards from the working directory and the executable looking for
    /// `vendor/Cura` and `vendor/fdm_materials`, unless the environment points
    /// somewhere else.
    pub fn discover() -> Result<Self, Error> {
        if let (Some(cura), Some(materials)) = (
            env::var_os("SHALE_CURA_DIR"),
            env::var_os("SHALE_MATERIALS_DIR"),
        ) {
            return Ok(Self {
                cura: PathBuf::from(cura),
                materials: PathBuf::from(materials),
            });
        }
        let mut candidates = Vec::new();
        push_ancestors(&mut candidates, env::current_dir().ok());
        push_ancestors(
            &mut candidates,
            env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf)),
        );
        for vendor in candidates {
            let cura = vendor.join("Cura");
            let materials = vendor.join("fdm_materials");
            if cura.join("resources").join("definitions").is_dir() && materials.is_dir() {
                return Ok(Self { cura, materials });
            }
        }
        Err(Error::message(
            "Cura resources were not found. Run scripts/fetch-resources.sh, or set SHALE_CURA_DIR and SHALE_MATERIALS_DIR.",
        ))
    }
}

fn push_ancestors(out: &mut Vec<PathBuf>, mut dir: Option<PathBuf>) {
    for _ in 0..8 {
        let Some(current) = dir.clone() else {
            break;
        };
        out.push(current.join("vendor"));
        dir = current.parent().map(Path::to_path_buf);
    }
}

#[derive(Clone, Debug)]
pub struct Catalog {
    pub paths: ResourcePaths,
    pub(crate) definitions: HashMap<String, Definition>,
    pub(crate) instances: Vec<InstanceFile>,
    pub(crate) materials: Vec<Material>,
    pub load_warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PrinterInfo {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub preferred_quality_type: String,
    pub preferred_variant: String,
    pub preferred_material: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct VariantInfo {
    pub name: String,
    pub nozzle_size: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QualityInfo {
    pub quality_type: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MaterialInfo {
    pub id: String,
    pub label: String,
    pub color: String,
    pub diameter: f64,
}

impl Catalog {
    pub fn open(paths: ResourcePaths) -> Result<Self, Error> {
        let resources = paths.resources();
        let mut warnings = Vec::new();
        let mut definitions = HashMap::new();
        load_definitions(
            &resources.join("definitions"),
            false,
            &mut definitions,
            &mut warnings,
        );
        load_definitions(
            &resources.join("extruders"),
            true,
            &mut definitions,
            &mut warnings,
        );
        if !definitions.contains_key("fdmprinter") {
            return Err(Error::message(format!(
                "fdmprinter.def.json is missing under {}",
                resources.display()
            )));
        }
        let mut instances = Vec::new();
        for folder in ["quality", "variants", "intent"] {
            load_instances(&resources.join(folder), &mut instances, &mut warnings);
        }
        let mut materials = Vec::new();
        load_materials(&paths.materials, &mut materials, &mut warnings);
        inherit_metadata(&mut definitions);
        Ok(Self {
            paths,
            definitions,
            instances,
            materials,
            load_warnings: warnings,
        })
    }

    pub fn printers(&self) -> Vec<PrinterInfo> {
        let mut printers: Vec<PrinterInfo> = self
            .definitions
            .values()
            .filter(|definition| {
                definition.visible
                    && !definition.is_extruder
                    && definition.id != "fdmprinter"
                    && definition.id != "fdmextruder"
            })
            .map(|definition| PrinterInfo {
                id: definition.id.clone(),
                name: definition.name.clone(),
                manufacturer: definition.manufacturer.clone(),
                preferred_quality_type: definition
                    .preferred_quality_type
                    .clone()
                    .unwrap_or_else(|| "normal".to_string()),
                preferred_variant: definition
                    .preferred_variant_name
                    .clone()
                    .unwrap_or_else(|| "0.4mm Nozzle".to_string()),
                preferred_material: definition
                    .preferred_material
                    .clone()
                    .unwrap_or_else(|| "generic_pla".to_string()),
            })
            .collect();
        printers.sort_by(|a, b| {
            a.manufacturer
                .cmp(&b.manufacturer)
                .then(a.name.cmp(&b.name))
        });
        printers
    }

    pub fn printer(&self, id: &str) -> Option<&Definition> {
        self.definitions.get(id)
    }

    /// Label and description for a setting, after definition inheritance.
    pub fn setting_text(&self, printer_id: &str, key: &str) -> (Option<String>, Option<String>) {
        let Ok(merged) = self.merged_settings(printer_id) else {
            return (None, None);
        };
        let Some(setting) = merged.get(key) else {
            return (None, None);
        };
        (setting.label.clone(), setting.description.clone())
    }

    pub fn variants_for(&self, printer_id: &str) -> Vec<VariantInfo> {
        let Some(ancestry) = self.ancestry_ids(printer_id) else {
            return Vec::new();
        };
        let mut by_name: HashMap<String, VariantInfo> = HashMap::new();
        let mut ranked: Vec<&InstanceFile> = self
            .instances
            .iter()
            .filter(|instance| instance.type_name == "variant")
            .filter(|instance| ancestry.iter().any(|id| id == &instance.definition))
            .collect();
        ranked.sort_by_key(|instance| specificity(&instance.definition, &ancestry));
        for instance in ranked {
            let nozzle = instance
                .values
                .get("machine_nozzle_size")
                .and_then(|value| value.trim().trim_start_matches('=').parse().ok());
            by_name.insert(
                instance.name.clone(),
                VariantInfo {
                    name: instance.name.clone(),
                    nozzle_size: nozzle,
                },
            );
        }
        let mut variants: Vec<_> = by_name.into_values().collect();
        variants.sort_by(|a, b| {
            a.nozzle_size
                .partial_cmp(&b.nozzle_size)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.name.cmp(&b.name))
        });
        variants
    }

    pub fn qualities_for(&self, printer_id: &str) -> Vec<QualityInfo> {
        let Some(ancestry) = self.ancestry_ids(printer_id) else {
            return Vec::new();
        };
        let mut by_type: HashMap<String, (usize, QualityInfo)> = HashMap::new();
        for instance in &self.instances {
            if instance.type_name != "quality" {
                continue;
            }
            let Some(quality_type) = &instance.quality_type else {
                continue;
            };
            if !ancestry.iter().any(|id| id == &instance.definition) {
                continue;
            }
            let rank = specificity(&instance.definition, &ancestry);
            let info = QualityInfo {
                quality_type: quality_type.clone(),
                name: if instance.name.is_empty() {
                    quality_type.clone()
                } else {
                    instance.name.clone()
                },
            };
            match by_type.get(quality_type) {
                Some((existing, _)) if *existing > rank => {}
                _ => {
                    by_type.insert(quality_type.clone(), (rank, info));
                }
            }
        }
        let mut qualities: Vec<_> = by_type.into_values().map(|(_, info)| info).collect();
        qualities.sort_by(|a, b| a.name.cmp(&b.name));
        qualities
    }

    pub fn materials_for(&self, diameter: f64) -> Vec<MaterialInfo> {
        let mut materials: Vec<MaterialInfo> = self
            .materials
            .iter()
            .filter(|material| (material.diameter - diameter).abs() < 0.4)
            .map(|material| MaterialInfo {
                id: material.id.clone(),
                label: format!(
                    "{} {} ({:.2} mm)",
                    material.brand, material.name, material.diameter
                ),
                color: material.color.clone(),
                diameter: material.diameter,
            })
            .collect();
        materials.sort_by(|a, b| a.label.cmp(&b.label));
        materials
    }

    pub(crate) fn ancestry_ids(&self, id: &str) -> Option<Vec<String>> {
        let mut chain = Vec::new();
        let mut current = Some(id.to_string());
        for _ in 0..32 {
            let Some(id) = current else {
                chain.reverse();
                return Some(chain);
            };
            let definition = self.definitions.get(&id)?;
            current = definition.inherits.clone();
            chain.push(id);
        }
        None
    }

    pub(crate) fn merged_settings(&self, id: &str) -> Result<HashMap<String, RawSetting>, Error> {
        let ancestry = self.ancestry_ids(id).ok_or_else(|| {
            Error::message(format!(
                "definition '{id}' is missing or its inheritance loops"
            ))
        })?;
        let mut merged: HashMap<String, RawSetting> = HashMap::new();
        for id in ancestry {
            let Some(definition) = self.definitions.get(&id) else {
                continue;
            };
            for (key, setting) in &definition.settings {
                merged.entry(key.clone()).or_default().merge(setting);
            }
        }
        Ok(merged)
    }
}

pub(crate) fn specificity(definition_id: &str, ancestry: &[String]) -> usize {
    ancestry
        .iter()
        .position(|id| id == definition_id)
        .unwrap_or(0)
}

/// Metadata such as the preferred quality and the extruder train often lives on
/// a hidden base definition (`creality_base`), not on the visible machine.
fn inherit_metadata(definitions: &mut HashMap<String, Definition>) {
    let snapshot: HashMap<String, Definition> = definitions.clone();
    for id in snapshot.keys() {
        let mut quality_definition = None;
        let mut preferred_quality = None;
        let mut preferred_variant = None;
        let mut preferred_material = None;
        let mut manufacturer = None;
        let mut trains = None;
        let mut current = Some(id.clone());
        for _ in 0..32 {
            let Some(next) = current else { break };
            let Some(definition) = snapshot.get(&next) else {
                break;
            };
            if quality_definition.is_none() {
                quality_definition.clone_from(&definition.quality_definition);
            }
            if preferred_quality.is_none() {
                preferred_quality.clone_from(&definition.preferred_quality_type);
            }
            if preferred_variant.is_none() {
                preferred_variant.clone_from(&definition.preferred_variant_name);
            }
            if preferred_material.is_none() {
                preferred_material.clone_from(&definition.preferred_material);
            }
            if manufacturer.is_none() && !definition.manufacturer.is_empty() {
                manufacturer = Some(definition.manufacturer.clone());
            }
            if trains.is_none() && !definition.extruder_trains.is_empty() {
                trains = Some(definition.extruder_trains.clone());
            }
            current = definition.inherits.clone();
        }
        let Some(definition) = definitions.get_mut(id) else {
            continue;
        };
        if definition.quality_definition.is_none() {
            definition.quality_definition = quality_definition;
        }
        if definition.preferred_quality_type.is_none() {
            definition.preferred_quality_type = preferred_quality;
        }
        if definition.preferred_variant_name.is_none() {
            definition.preferred_variant_name = preferred_variant;
        }
        if definition.preferred_material.is_none() {
            definition.preferred_material = preferred_material;
        }
        if definition.manufacturer.is_empty() {
            if let Some(name) = manufacturer {
                definition.manufacturer = name;
            }
        }
        if definition.extruder_trains.is_empty() {
            if let Some(found) = trains {
                definition.extruder_trains = found;
            }
        }
    }
}

fn load_definitions(
    dir: &Path,
    extruder: bool,
    into: &mut HashMap<String, Definition>,
    warnings: &mut Vec<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".def.json") {
            continue;
        }
        match definition::load_definition(&path, extruder) {
            Ok(definition) => {
                into.insert(definition.id.clone(), definition);
            }
            Err(err) => warnings.push(err.to_string()),
        }
    }
}

fn load_instances(dir: &Path, into: &mut Vec<InstanceFile>, warnings: &mut Vec<String>) {
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    for path in files {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".cfg") {
            continue;
        }
        match instance::load_instance(&path) {
            Ok(instance) => into.push(instance),
            Err(err) => {
                if warnings.len() < 40 {
                    warnings.push(err.to_string());
                }
            }
        }
    }
}

fn load_materials(dir: &Path, into: &mut Vec<Material>, warnings: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            warnings.push(format!("materials: {err}"));
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".fdm_material") {
            continue;
        }
        match material::load_material(&path) {
            Ok(material) => into.push(material),
            Err(err) => {
                if warnings.len() < 40 {
                    warnings.push(err.to_string());
                }
            }
        }
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}
