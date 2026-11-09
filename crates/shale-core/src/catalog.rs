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

    /// Find Cura's definitions and the material library.
    ///
    /// Order: `SHALE_CURA_DIR` and `SHALE_MATERIALS_DIR`, then `vendor/` next
    /// to the working directory or the executable, then an installed
    /// UltiMaker Cura 5.x (`share/cura/resources`, and on Windows the
    /// `UltiMaker Cura*` folder under Program Files). The error names every
    /// path that was tried.
    pub fn discover() -> Result<Self, Error> {
        let mut tried = Vec::new();
        if let (Some(cura), Some(materials)) = (
            env::var_os("SHALE_CURA_DIR"),
            env::var_os("SHALE_MATERIALS_DIR"),
        ) {
            let paths = Self {
                cura: PathBuf::from(cura),
                materials: PathBuf::from(materials),
            };
            tried.push(format!(
                "SHALE_CURA_DIR={} SHALE_MATERIALS_DIR={}",
                paths.cura.display(),
                paths.materials.display()
            ));
            if paths.has_definitions() {
                return Ok(paths);
            }
        } else if env::var_os("SHALE_CURA_DIR").is_some()
            || env::var_os("SHALE_MATERIALS_DIR").is_some()
        {
            tried.push("SHALE_CURA_DIR and SHALE_MATERIALS_DIR must both be set".to_string());
        }

        let mut roots = Vec::new();
        push_ancestors(&mut roots, env::current_dir().ok());
        push_ancestors(
            &mut roots,
            env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf)),
        );
        for vendor in roots {
            let cura = vendor.join("Cura");
            let materials = vendor.join("fdm_materials");
            tried.push(format!(
                "{} (materials {})",
                cura.display(),
                materials.display()
            ));
            let paths = Self { cura, materials };
            if paths.has_definitions() {
                return Ok(paths);
            }
        }

        for paths in installed_cura_resources() {
            tried.push(format!(
                "installed Cura {} (materials {})",
                paths.cura.display(),
                paths.materials.display()
            ));
            if paths.has_definitions() {
                return Ok(paths);
            }
        }

        Err(Error::message(format!(
            "Cura printer definitions were not found, so the printer list is empty. Paths tried:\n{}\nOn Windows run scripts/fetch-resources.ps1, or install UltiMaker Cura 5.x. On Linux or macOS run scripts/fetch-resources.sh. Both environment variables can point at an existing tree: SHALE_CURA_DIR and SHALE_MATERIALS_DIR.",
            if tried.is_empty() {
                "(none)".to_string()
            } else {
                tried.join("\n")
            }
        )))
    }

    fn has_definitions(&self) -> bool {
        self.resources()
            .join("definitions")
            .join("fdmprinter.def.json")
            .is_file()
    }
}

/// UltiMaker Cura 5.x installs its definitions under `share/cura/resources`.
/// Materials ship in that tree as well; the separate fdm_materials checkout is
/// only needed for the fetched copy.
fn installed_cura_resources() -> Vec<ResourcePaths> {
    let mut found = Vec::new();
    for root in cura_install_roots() {
        if !root.is_dir() {
            continue;
        }
        let direct = root.join("share").join("cura");
        push_cura_tree(&direct, &mut found);
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.to_ascii_lowercase().starts_with("ultimaker cura") {
                push_cura_tree(&entry.path().join("share").join("cura"), &mut found);
                push_cura_tree(&entry.path(), &mut found);
            }
        }
    }
    found
}

fn cura_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(value) = env::var_os(key) {
            roots.push(PathBuf::from(value));
        }
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        roots.push(local.join("Programs"));
    }
    roots.push(PathBuf::from("/usr/share"));
    roots.push(PathBuf::from("/usr/local/share"));
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".local").join("share"));
    }
    roots
}

fn push_cura_tree(cura_root: &Path, found: &mut Vec<ResourcePaths>) {
    let resources = if cura_root.join("resources").join("definitions").is_dir() {
        cura_root.join("resources")
    } else if cura_root.join("definitions").is_dir() {
        cura_root.to_path_buf()
    } else {
        return;
    };
    let materials = if resources.join("materials").is_dir() {
        resources.join("materials")
    } else {
        cura_root.join("fdm_materials")
    };
    // `ResourcePaths::resources` looks for `<cura>/resources` first. Point
    // `cura` at the parent of the definitions folder when that parent is the
    // resources directory itself.
    let cura = if resources.file_name().and_then(|n| n.to_str()) == Some("resources") {
        resources
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(resources)
    } else {
        resources
    };
    found.push(ResourcePaths { cura, materials });
}

#[cfg(test)]
mod discover_tests {
    use super::*;

    #[test]
    fn an_installed_tree_is_recognised_and_a_miss_names_the_path() {
        let root = std::env::temp_dir().join(format!("shale-cura-tree-{}", std::process::id()));
        let cura = root.join("share").join("cura");
        let definitions = cura.join("resources").join("definitions");
        fs::create_dir_all(&definitions).unwrap();
        fs::write(definitions.join("fdmprinter.def.json"), b"{}").unwrap();
        let mut found = Vec::new();
        push_cura_tree(&cura, &mut found);
        assert_eq!(found.len(), 1, "looked at {}", cura.display());
        assert!(found[0].has_definitions());
        let _ = fs::remove_dir_all(&root);

        let missing = ResourcePaths::discover();
        if let Err(err) = missing {
            let text = err.to_string();
            assert!(
                text.contains("Paths tried"),
                "the empty-list failure must name the paths: {text}"
            );
        }
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
    /// Printer ids that came from an Orca-family library rather than Cura.
    pub(crate) external: std::collections::HashSet<String>,
    /// Material ids from the same library, so a filament list can tell the two
    /// sources apart without guessing from the brand string.
    pub(crate) external_materials: std::collections::HashSet<String>,
    /// Start and end templates from an Orca-family machine profile, by machine
    /// id and nozzle.
    pub(crate) gcode: Vec<(String, String, crate::bambu::MachineGcode)>,
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
    /// `"cura"` for the Cura resource tree, `"bambu"` for an Orca-family
    /// profile library. The desktop app groups and badges on this.
    pub source: String,
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
    /// Grams per cubic centimetre, when the profile says. Bambu's filaments
    /// carry it; Cura's materials do not, and a weight invented from a guess
    /// would be worse than no weight at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density: Option<f64>,
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
        // Bambu Studio's own tree is a second, independent profile library. Its
        // entries are converted into the same shapes Cura's resources produce,
        // so variants, qualities, materials and the resolver need no special
        // case; only the `source` tag differs.
        let library = crate::bambu::Library::load();
        let gcode = library.gcode.clone();
        let mut external = std::collections::HashSet::new();
        for definition in library.definitions {
            external.insert(definition.id.clone());
            definitions.entry(definition.id.clone()).or_insert(definition);
        }
        // Bambu's extruder profiles are self-contained, but CuraEngine asks for
        // base extruder values by name (nozzle offsets, cooling fan number, ...)
        // that Cura's `fdmextruder` supplies. Copy them over rather than hand
        // listing keys here, so the next one is not a mystery.
        if let Some(base) = definitions.get("fdmextruder").map(|d| d.settings.clone()) {
            for definition in definitions
                .values_mut()
                .filter(|d| d.is_extruder && external.contains(&d.id))
            {
                for (key, setting) in &base {
                    definition
                        .settings
                        .entry(key.clone())
                        .or_insert_with(|| setting.clone());
                }
            }
        }
        instances.extend(library.instances);
        let external_materials: std::collections::HashSet<String> = library
            .materials
            .iter()
            .map(|material| material.id.clone())
            .collect();
        materials.extend(library.materials);
        inherit_metadata(&mut definitions);
        Ok(Self {
            paths,
            definitions,
            instances,
            materials,
            external,
            external_materials,
            gcode,
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
                source: if self.external.contains(&definition.id) {
                    "bambu".to_string()
                } else {
                    "cura".to_string()
                },
            })
            .collect();
        printers.sort_by(|a, b| {
            a.manufacturer
                .cmp(&b.manufacturer)
                .then(a.name.cmp(&b.name))
        });
        printers
    }

    /// The machine's own start and end sequence for a nozzle, if its profile
    /// library ships one. `None` is a machine whose start is Cura's, which is
    /// right for a Cura printer and wrong for a Bambu one.
    pub fn machine_gcode(&self, id: &str, nozzle: &str) -> Option<&crate::bambu::MachineGcode> {
        self.gcode
            .iter()
            .find(|(machine, variant, _)| {
                machine == id && (variant == nozzle || variant.starts_with(nozzle))
            })
            .map(|(_, _, gcode)| gcode)
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
                density: material.settings.get("material_density").copied(),
            })
            .collect();
        materials.sort_by(|a, b| a.label.cmp(&b.label));
        materials
    }

    /// Materials that belong on this machine.
    ///
    /// Cura's library is shared across the whole catalogue and the UI has always
    /// offered it as one list. An Orca-family library is different: every
    /// filament names the printers it was tuned for, and there are well over a
    /// thousand of them, so they are filtered per machine instead of pooled.
    pub fn materials_for_printer(&self, printer_id: &str, diameter: f64) -> Vec<MaterialInfo> {
        let external_printer = self.external.contains(printer_id);
        let materials: Vec<MaterialInfo> = self
            .materials
            .iter()
            .filter(|material| (material.diameter - diameter).abs() < 0.4)
            .filter(|material| {
                let from_library = self.external_materials.contains(&material.id);
                if !external_printer {
                    return !from_library;
                }
                if !from_library {
                    return false;
                }
                material.machines.iter().any(|machine| {
                    machine
                        .products
                        .iter()
                        .any(|product| product == printer_id || product == "*")
                })
            })
            .map(|material| MaterialInfo {
                id: material.id.clone(),
                label: material.name.clone(),
                color: material.color.clone(),
                diameter: material.diameter,
                density: material.settings.get("material_density").copied(),
            })
            .collect();
        // The library already orders these (PLA first, then the rest) and that
        // order is more useful here than an alphabetical one.
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
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    if files.is_empty() {
        warnings.push(format!("materials: nothing under {}", dir.display()));
        return;
    }
    for path in files {
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
            if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
                continue;
            }
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}
