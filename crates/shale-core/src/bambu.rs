//! Printer, filament and process profiles from Orca-family slicers.
//!
//! Bambu Studio (and Orca, which shares the fork) ship their profile library as
//! plain JSON: `resources/profiles/<VENDOR>/{machine,filament,process}/*.json`.
//! A profile is a thin file that names a parent with `inherits`; the values
//! that matter live further up the chain. This module walks those chains and
//! converts the result into the same `Definition` / `InstanceFile` / `Material`
//! values the Cura resources produce, so the rest of Shale does not need to
//! know where a printer came from.
//!
//! The mapping is deliberately conservative: machine size, nozzle size,
//! temperatures, layer height, walls, infill and support are translated, and
//! anything without a Cura equivalent is left behind. An exported G-code file
//! is Marlin-flavoured, not a Bambu job.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[cfg(test)]
static TEST_PROFILES_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

#[cfg(test)]
pub(crate) fn set_test_profiles_root(dir: Option<PathBuf>) {
    if let Ok(mut guard) = TEST_PROFILES_ROOT.write() {
        *guard = dir;
    }
}

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::catalog_cache::{load_bambu_library, store_bambu_library_async};
use crate::definition::{Definition, RawSetting};
use crate::instance::InstanceFile;
use crate::material::{MachineMaterial, Material};

/// Vendors read by default. Bambu Studio's own tree is the one that is
/// installed on a machine that slices for a Bambu printer; the others share the
/// schema and can be switched on with `SHALE_ORCA_VENDORS`.
const DEFAULT_VENDORS: &str = "BBL";

/// More nozzles than this per model is noise in a dropdown.
const MAX_VARIANTS: usize = 8;
/// Filaments shown per printer.
const MAX_MATERIALS: usize = 40;
/// Depth guard for `inherits` chains; the deepest real chain is 3.
const MAX_DEPTH: usize = 12;

/// The machine's own start and end G-code, still holding its placeholders.
/// Bambu Studio keeps them in files beside the machine profile rather than in
/// the profile itself, and they are templates, not G-code: see
/// `crate::template` for the filling.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineGcode {
    pub start: String,
    pub end: String,
}

/// Read `<profile file> template machine_start_gcode.json` and its end
/// counterpart. Either half missing is no template at all: half a sequence is
/// worse than the flavour's defaults.
fn machine_gcode(dir: &Path, profile_file: &str) -> Option<MachineGcode> {
    let read = |kind: &str| -> Option<String> {
        let path = dir.join(format!("{profile_file} template {kind}.json"));
        let text = std::fs::read_to_string(path).ok()?;
        let parsed: Json = serde_json::from_str(&text).ok()?;
        let value = parsed.get(kind)?;
        match value {
            Json::String(text) => Some(text.clone()),
            Json::Array(items) => Some(
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(""),
            ),
            _ => None,
        }
    };
    Some(MachineGcode {
        start: read("machine_start_gcode")?,
        end: read("machine_end_gcode")?,
    })
}

#[derive(Clone)]
pub struct Library {
    pub definitions: Vec<Definition>,
    pub instances: Vec<InstanceFile>,
    pub materials: Vec<Material>,
    /// `(machine id, nozzle, gcode)`, one entry per nozzle the profile has a
    /// template for.
    pub gcode: Vec<(String, String, MachineGcode)>,
}

impl Library {
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    /// Load from the configured profile root, or an empty library when no
    /// slicer is installed. Never fails: a missing library is normal.
    pub fn load() -> Self {
        let Some(root) = profiles_root() else {
            return Self::empty();
        };
        if let Some(library) = load_bambu_library(&root) {
            return library;
        }
        let library = Self::from_root(Some(root.clone()));
        store_bambu_library_async(root, library.clone());
        library
    }

    pub fn from_root(root: Option<PathBuf>) -> Self {
        let Some(root) = root else {
            return Self::empty();
        };
        if !root.is_dir() {
            return Self::empty();
        }
        let vendors = vendor_list();
        let mut library = Self::empty();
        for code in vendors {
            let dir = root.join(&code);
            if !dir.is_dir() {
                continue;
            }
            library.read_vendor(&code, &dir);
        }
        library
    }

    fn empty() -> Self {
        Self {
            definitions: Vec::new(),
            instances: Vec::new(),
            materials: Vec::new(),
            gcode: Vec::new(),
        }
    }

    fn read_vendor(&mut self, code: &str, dir: &Path) {
        let vendor = display_vendor(code);
        let machines = read_machines(dir);
        if machines.is_empty() {
            return;
        }
        // Model name -> every machine profile file that describes it, so a
        // filament that only lists the 0.6 nozzle still shows up for the 0.4.
        let mut by_model: HashMap<String, Vec<Profile>> = HashMap::new();
        for profile in machines {
            by_model
                .entry(profile.model.clone())
                .or_default()
                .push(profile);
        }
        let mut models: Vec<String> = by_model.keys().cloned().collect();
        models.sort();

        // Both folders are large and every model reads from them, so they are
        // parsed once here. Doing it inside the loop cost eight seconds.
        let processes = profiles_in(&dir.join("process"));
        let filament_profiles = profiles_in(&dir.join("filament"));

        for model in models {
            let group = &by_model[&model];
            let Some(base) = group
                .iter()
                .find(|profile| profile.base_nozzle())
                .or_else(|| group.first())
            else {
                continue;
            };
            let id = slug(&model);
            let definition_index = self.definitions.len();
            let names: Vec<String> = group.iter().map(|profile| profile.file.clone()).collect();
            let mut nozzles: Vec<String> =
                group.iter().map(|profile| profile.nozzle.clone()).collect();
            nozzles.sort_by(|a, b| {
                number(a)
                    .partial_cmp(&number(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            nozzles.dedup();
            nozzles.truncate(MAX_VARIANTS);

            self.definitions
                .push(printer_definition(&id, &model, vendor, base));
            self.definitions
                .push(extruder_definition(&id, &model, base));
            for nozzle in &nozzles {
                self.instances.push(variant_instance(&id, nozzle));
            }
            // The start and end sequences are templates in the machine folder,
            // named after the profile they belong to. A vendor that ships none
            // keeps the flavour's own defaults.
            let machine_dir = dir.join("machine");
            for profile in group {
                if let Some(gcode) = machine_gcode(&machine_dir, &profile.file) {
                    self.gcode.push((id.clone(), profile.nozzle.clone(), gcode));
                }
            }
            self.instances.extend(quality_instances(
                &processes,
                &id,
                &names,
                &base.default_quality,
                &nozzles,
                &base.nozzle,
            ));
            let first_material = self.materials.len();
            self.materials
                .extend(filaments(&filament_profiles, &id, vendor, &names, &model));
            // A printer with no preferred filament would resolve to Cura's
            // `generic_pla`, which is not in this library at all. The first PLA
            // in its own list is the honest default.
            let fresh = &self.materials[first_material..];
            let preferred = fresh
                .iter()
                .find(|material| {
                    material.name.eq_ignore_ascii_case("Bambu PLA Basic")
                        || material.name.eq_ignore_ascii_case("Generic PLA")
                })
                .or_else(|| {
                    fresh.iter().find(|material| {
                        let name = material.name.to_ascii_uppercase();
                        name.contains("PLA") && !name.contains("SUPPORT")
                    })
                });
            if let Some(material) = preferred {
                self.definitions[definition_index].preferred_material = Some(material.id.clone());
            }
        }
    }
}

/// Directory that holds Orca/Bambu `resources/profiles`, from env or common installs.
pub fn profiles_root() -> Option<PathBuf> {
    #[cfg(test)]
    if let Ok(guard) = TEST_PROFILES_ROOT.read() {
        if let Some(dir) = guard.as_ref() {
            return Some(dir.clone());
        }
    }
    if let Some(dir) = std::env::var_os("SHALE_ORCA_DIR") {
        return Some(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::var_os("SHALE_BAMBU_DIR") {
        return Some(PathBuf::from(dir));
    }
    let mut candidates = vec![
        PathBuf::from("C:/Program Files/Bambu Studio/resources/profiles"),
        PathBuf::from("/Applications/BambuStudio.app/Contents/Resources/profiles"),
        PathBuf::from("/usr/share/BambuStudio/resources/profiles"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join(".config/BambuStudio/system/profiles"));
        candidates.push(home.join("Library/Application Support/BambuStudio/system/profiles"));
    }
    candidates.into_iter().find(|path| path.is_dir())
}

fn vendor_list() -> Vec<String> {
    let raw = std::env::var("SHALE_ORCA_VENDORS").unwrap_or_else(|_| DEFAULT_VENDORS.to_string());
    let mut vendors: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(|code| code.to_string())
        .collect();
    vendors.dedup();
    vendors
}

fn display_vendor(code: &str) -> &str {
    match code {
        "BBL" => "Bambu Lab",
        "Creality" => "Creality",
        "Anycubic" => "Anycubic",
        "Elegoo" => "Elegoo",
        "Anker" => "Anker",
        other => other,
    }
}

struct Profile {
    /// File stem, which is also the name other profiles use in
    /// `compatible_printers`.
    file: String,
    model: String,
    nozzle: String,
    default_quality: String,
    width: f64,
    depth: f64,
    height: f64,
    centered: bool,
    /// Merged machine profile values (`inherits` applied).
    machine_settings: Json,
}

impl Profile {
    fn base_nozzle(&self) -> bool {
        (number(&self.nozzle) - 0.4).abs() < 0.001
    }
}

fn read_machines(dir: &Path) -> Vec<Profile> {
    let mut out = Vec::new();
    for (stem, values) in profiles_in(&dir.join("machine")) {
        let Some(model) = text(values.get("printer_model")) else {
            continue;
        };
        let Some(area) = values.get("printable_area").and_then(polygon_extent) else {
            continue;
        };
        let nozzle = text(values.get("printer_variant"))
            .or_else(|| first_text(values.get("nozzle_diameter")))
            .unwrap_or_else(|| "0.4".to_string());
        let height = values
            .get("printable_height")
            .and_then(number_value)
            .unwrap_or(200.0);
        let default_quality = text(values.get("default_print_profile")).unwrap_or_default();
        out.push(Profile {
            file: stem,
            model,
            nozzle,
            default_quality,
            width: area.0,
            depth: area.1,
            height,
            centered: area.2,
            machine_settings: values.clone(),
        });
    }
    out
}

/// Every profile in `dir` with its `inherits` chain applied, keyed by file stem.
///
/// The filament folder alone holds well over a thousand files, and each one
/// names a parent, so lookups must not rescan the directory. One index and one
/// chain cache per folder keeps the whole library read in well under a second.
fn profiles_in(dir: &Path) -> Vec<(String, Json)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut index: HashMap<String, PathBuf> = HashMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        // `... template machine_start_gcode.json` fragments carry no profile.
        if stem.contains(" template ") {
            continue;
        }
        index.insert(stem.to_ascii_lowercase(), path);
    }
    let mut stems: Vec<(String, String)> = index
        .iter()
        .filter_map(|(lower, path)| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| (stem.to_string(), lower.clone()))
        })
        .collect();
    stems.sort();
    let mut memo: HashMap<String, Json> = HashMap::new();
    let mut out = Vec::with_capacity(stems.len());
    for (stem, lower) in stems {
        if let Some(values) = resolve_chain(&index, &mut memo, &lower, 0) {
            out.push((stem, values));
        }
    }
    out
}

/// Read `stem.json` and merge everything it inherits, parent first.
fn resolve_chain(
    index: &HashMap<String, PathBuf>,
    memo: &mut HashMap<String, Json>,
    stem: &str,
    depth: usize,
) -> Option<Json> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let Some(cached) = memo.get(stem) {
        return Some(cached.clone());
    }
    let path = index.get(stem)?;
    let text = fs::read_to_string(path).ok()?;
    let value: Json = serde_json::from_str(&text).ok()?;
    let parent = value
        .get("inherits")
        .and_then(|name| name.as_str())
        .and_then(|name| resolve_chain(index, memo, &name.to_ascii_lowercase(), depth + 1));
    let mut merged = parent.unwrap_or_else(|| Json::Object(Default::default()));
    merge_json(&mut merged, &value);
    memo.insert(stem.to_string(), merged.clone());
    Some(merged)
}

fn merge_json(into: &mut Json, from: &Json) {
    match (into, from) {
        (Json::Object(target), Json::Object(source)) => {
            for (key, value) in source {
                match target.get_mut(key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target, source) => *target = source.clone(),
    }
}

fn printer_definition(id: &str, model: &str, vendor: &str, base: &Profile) -> Definition {
    let mut settings = HashMap::new();
    let mut put = |key: &str, ty: &str, value: Json| {
        settings.insert(
            key.to_string(),
            RawSetting {
                ty: Some(ty.to_string()),
                default_value: Some(value),
                ..Default::default()
            },
        );
    };
    put("machine_width", "float", Json::from(base.width));
    put("machine_depth", "float", Json::from(base.depth));
    put("machine_height", "float", Json::from(base.height));
    put("machine_center_is_zero", "bool", Json::from(base.centered));
    put("machine_shape", "enum", Json::from("rectangular"));
    put("machine_extruder_count", "int", Json::from(1));
    put("material_diameter", "float", Json::from(1.75));
    put("machine_name", "str", Json::from(model));
    // CuraEngine's Marlin flavour is what the exported G-code already targets;
    // Bambu's own start G-code is not Marlin and is not used here.
    put("machine_gcode_flavor", "enum", Json::from("marlin"));
    put("machine_heated_bed", "bool", Json::from(true));
    for (key, text) in machine_values_for_cura(&base.machine_settings) {
        if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("false") {
            put(&key, "bool", Json::from(text.eq_ignore_ascii_case("true")));
        } else if let Ok(number) = text.parse::<f64>() {
            put(&key, "float", Json::from(number));
        } else {
            put(&key, "str", Json::from(text));
        }
    }
    // Bambu's start G-code is not valid Marlin; leave the Cura default in place
    // rather than shipping a template full of {placeholders} the engine cannot
    // resolve for this machine.
    Definition {
        id: id.to_string(),
        // The window prints the manufacturer next to the name, so a machine
        // called "Bambu Lab A1" under "Bambu Lab" would read twice.
        name: short_model(model, vendor),
        // Cura's base machine carries the globals CuraEngine insists on
        // (`mesh_rotation_matrix` among them); `Catalog::open` adds a stand-in
        // when the Cura tree is not installed.
        inherits: Some("fdmprinter".to_string()),
        visible: true,
        manufacturer: vendor.to_string(),
        quality_definition: Some(id.to_string()),
        preferred_quality_type: Some(slug(&base.default_quality)),
        preferred_variant_name: Some(format!("{} mm", base.nozzle)),
        preferred_material: None,
        extruder_trains: vec![(0, format!("{id}_extruder_0"))],
        is_extruder: false,
        settings,
    }
}

fn short_model(model: &str, vendor: &str) -> String {
    let lower = model.to_ascii_lowercase();
    let vendor_lower = vendor.to_ascii_lowercase();
    if lower.starts_with(&vendor_lower) {
        let rest = model[vendor.len()..].trim_start();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    model.to_string()
}

fn extruder_definition(id: &str, model: &str, base: &Profile) -> Definition {
    let mut settings = HashMap::new();
    settings.insert(
        "machine_nozzle_size".to_string(),
        RawSetting {
            ty: Some("float".to_string()),
            default_value: Some(Json::from(number(&base.nozzle))),
            ..Default::default()
        },
    );
    settings.insert(
        "material_diameter".to_string(),
        RawSetting {
            ty: Some("float".to_string()),
            default_value: Some(Json::from(1.75)),
            ..Default::default()
        },
    );
    // The rest of what Cura's `fdmextruder` would have supplied. It ships in
    // `definitions/`, not `extruders/`, so Shale's extruder loader never sees
    // it and CuraEngine asks for these by name.
    for (key, value) in [
        ("machine_nozzle_offset_x", Json::from(0.0)),
        ("machine_nozzle_offset_y", Json::from(0.0)),
        ("extruder_nr", Json::from(0)),
    ] {
        settings.insert(
            key.to_string(),
            RawSetting {
                default_value: Some(value),
                ..Default::default()
            },
        );
    }
    settings.insert(
        "machine_nozzle_id".to_string(),
        RawSetting {
            ty: Some("str".to_string()),
            default_value: Some(Json::from("Default")),
            ..Default::default()
        },
    );
    Definition {
        id: format!("{id}_extruder_0"),
        name: format!("{model} extruder 0"),
        // Self-contained: Bambu's extruder chain is already flattened into
        // `settings`, and Cura 5.13 keeps `fdmextruder.def.json` in
        // `definitions/` where the extruder loader never sees it.
        inherits: None,
        visible: false,
        manufacturer: "Bambu Lab".to_string(),
        quality_definition: None,
        preferred_quality_type: None,
        preferred_variant_name: None,
        preferred_material: None,
        extruder_trains: Vec::new(),
        is_extruder: true,
        settings,
    }
}

fn variant_instance(printer: &str, nozzle: &str) -> InstanceFile {
    let mut values = HashMap::new();
    values.insert("machine_nozzle_size".to_string(), nozzle.to_string());
    InstanceFile {
        name: format!("{nozzle} mm"),
        definition: printer.to_string(),
        type_name: "variant".to_string(),
        quality_type: None,
        variant: None,
        material: None,
        global_quality: false,
        weight: 0,
        intent_category: None,
        values,
    }
}

/// Slug tail for a nozzle diameter, e.g. `0.4` → `0_4_nozzle`, used to match
/// process file stems such as `0.20mm Standard @BBL A1 0.4 nozzle`.
pub fn nozzle_slug_tag(nozzle_diameter: &str) -> String {
    slug(&format!("{} nozzle", nozzle_diameter.trim()))
}

pub fn variant_nozzle_tag(variant_name: &str) -> Option<String> {
    let token = variant_name.split_whitespace().next()?;
    if token.is_empty() {
        return None;
    }
    Some(nozzle_slug_tag(token))
}

fn scalar_setting_string(value: &Json) -> Option<String> {
    if let Some(number) = number_value(value) {
        return Some(number_to_string(number));
    }
    text(Some(value))
}

fn number_to_string(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value}")
    }
}

fn insert_cura_scalar(out: &mut HashMap<String, String>, cura_key: &str, value: &Json) {
    if let Some(text) = scalar_setting_string(value) {
        out.insert(cura_key.to_string(), text);
    }
}

/// Bambu machine keys that CuraEngine understands for time estimation.
fn machine_values_for_cura(values: &Json) -> HashMap<String, String> {
    const MAP: &[(&str, &str)] = &[
        ("machine_max_acceleration_extruding", "machine_max_acceleration_e"),
        ("machine_max_acceleration_x", "machine_max_acceleration_x"),
        ("machine_max_acceleration_y", "machine_max_acceleration_y"),
        ("machine_max_acceleration_z", "machine_max_acceleration_z"),
        ("machine_max_jerk_e", "machine_max_jerk_e"),
        ("machine_max_speed_z", "machine_max_speed_z"),
    ];
    let Json::Object(map) = values else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for (bambu, cura) in MAP {
        if let Some(value) = map.get(*bambu) {
            insert_cura_scalar(&mut out, cura, value);
        }
    }
    out.insert("acceleration_enabled".to_string(), "True".to_string());
    out
}

/// Scalar entries from a merged process JSON (inherits already applied),
/// translated to Cura setting keys the resolver understands.
fn process_values_for_cura(values: &Json) -> HashMap<String, String> {
    const SKIP: &[&str] = &[
        "type",
        "name",
        "inherits",
        "from",
        "setting_id",
        "instantiation",
        "description",
        "compatible_printers",
        "compatible_prints",
        "setting_overrides",
        "sub_path",
    ];
    let Json::Object(map) = values else {
        return HashMap::new();
    };
    const MAP: &[(&str, &str)] = &[
        ("outer_wall_speed", "speed_wall_0"),
        ("inner_wall_speed", "speed_wall_x"),
        ("sparse_infill_speed", "speed_infill"),
        ("internal_solid_infill_speed", "speed_topbottom"),
        ("top_surface_speed", "speed_topbottom"),
        ("initial_layer_speed", "speed_layer_0"),
        ("initial_layer_infill_speed", "speed_infill"),
        ("travel_speed", "speed_travel"),
        ("travel_speed_z", "speed_z_hop"),
        ("support_speed", "speed_support"),
        ("bridge_speed", "speed_bridge"),
        ("default_acceleration", "acceleration_print"),
        ("outer_wall_acceleration", "acceleration_wall_0"),
        ("inner_wall_acceleration", "acceleration_wall_x"),
        ("initial_layer_acceleration", "acceleration_layer_0"),
        ("sparse_infill_acceleration", "acceleration_infill"),
        ("top_surface_acceleration", "acceleration_topbottom"),
        ("travel_acceleration", "acceleration_travel"),
    ];
    let mut out = HashMap::new();
    for (key, value) in map {
        if SKIP.contains(&key.as_str()) {
            continue;
        }
        if let Some((_, cura)) = MAP.iter().find(|(bambu, _)| *bambu == key.as_str()) {
            insert_cura_scalar(&mut out, cura, value);
            continue;
        }
        match key.as_str() {
            "wall_loops" => {
                if let Some(walls) = number_value(value) {
                    out.insert("wall_line_count".to_string(), format!("{}", walls.round()));
                }
            }
            "sparse_infill_density" => {
                if let Some(infill) = percent_value(value) {
                    out.insert("infill_sparse_density".to_string(), format!("{infill}"));
                }
            }
            "sparse_infill_pattern" => {
                if let Some(pattern) = text(Some(value)) {
                    out.insert("infill_pattern".to_string(), cura_infill(&pattern));
                }
            }
            "top_shell_layers" => {
                if let Some(layers) = number_value(value) {
                    out.insert("top_layers".to_string(), format!("{}", layers.round()));
                }
            }
            "bottom_shell_layers" => {
                if let Some(layers) = number_value(value) {
                    out.insert("bottom_layers".to_string(), format!("{}", layers.round()));
                }
            }
            "enable_support" => {
                if let Some(support) = truthy(value) {
                    out.insert(
                        "support_enable".to_string(),
                        if support { "True" } else { "False" }.to_string(),
                    );
                }
            }
            "initial_layer_print_height" | "initial_layer_height" => {
                if let Some(text) = scalar_setting_string(value) {
                    out.insert("layer_height_0".to_string(), text);
                }
            }
            "layer_height" => insert_cura_scalar(&mut out, "layer_height", value),
            _ => {}
        }
    }
    if let Some(speed) = out
        .get("speed_infill")
        .cloned()
        .or_else(|| out.get("speed_wall_0").cloned())
    {
        out.entry("speed_print".to_string()).or_insert(speed);
    }
    out.insert("acceleration_enabled".to_string(), "True".to_string());
    out
}

/// A process preset Bambu Studio would list, not an `instantiation: false` base.
fn process_is_selectable(values: &Json) -> bool {
    if let Some(flag) = values.get("instantiation") {
        return truthy(flag).unwrap_or(false);
    }
    // Visible presets carry a setting id; inherited-only rows do not.
    values.get("setting_id").is_some()
}

fn quality_instances(
    processes: &[(String, Json)],
    printer: &str,
    machine_names: &[String],
    default_quality: &str,
    nozzles: &[String],
    base_nozzle: &str,
) -> Vec<InstanceFile> {
    let mut out = Vec::new();
    for (stem, values) in processes {
        if !applies_to(values, machine_names) {
            continue;
        }
        if !process_is_selectable(values) {
            continue;
        }
        let stem_slug = slug(stem);
        let variant = if stem_slug.contains("_nozzle") {
            nozzles.iter().find_map(|nozzle| {
                let tag = nozzle_slug_tag(nozzle);
                stem_slug.ends_with(&tag).then(|| format!("{nozzle} mm"))
            })
        } else {
            nozzles
                .iter()
                .find(|nozzle| (number(nozzle) - number(base_nozzle)).abs() < 0.001)
                .map(|nozzle| format!("{nozzle} mm"))
        };
        let Some(variant) = variant else {
            continue;
        };
        if values.get("layer_height").and_then(number_value).is_none() {
            continue;
        }
        let settings = process_values_for_cura(values);
        let name = pretty_quality(stem);
        out.push(InstanceFile {
            name,
            definition: printer.to_string(),
            type_name: "quality".to_string(),
            quality_type: Some(stem_slug),
            variant: Some(variant),
            material: None,
            global_quality: true,
            weight: 0,
            intent_category: None,
            values: settings,
        });
    }
    // The machine's own default goes first, then the plain language profiles a
    // person actually recognises; the rest stay available behind them.
    out.sort_by_key(|instance| {
        let stem = instance.quality_type.clone().unwrap_or_default();
        let preferred = if !default_quality.is_empty() && slug(default_quality) == stem {
            0
        } else if stem.contains("standard") {
            1
        } else if stem.contains("fine") || stem.contains("optimal") || stem.contains("draft") {
            2
        } else {
            3
        };
        (preferred, instance.name.clone())
    });
    out
}

fn filaments(
    filaments: &[(String, Json)],
    printer: &str,
    vendor: &str,
    machine_names: &[String],
    model: &str,
) -> Vec<Material> {
    let mut out: Vec<Material> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for (stem, values) in filaments {
        // `fdm_filament_pla` and friends are the base profiles the Bambu
        // filaments inherit from, not filaments you can load.
        if stem.starts_with("fdm_") {
            continue;
        }
        if !applies_to(values, machine_names) {
            continue;
        }
        let Some(kind) = first_text(values.get("filament_type")) else {
            continue;
        };
        let name = text(values.get("name")).unwrap_or_else(|| stem.clone());
        let short = name.split(" @").next().unwrap_or(&name).to_string();
        // "Bambu PLA Basic" is stored as `bambu_lab_pla_basic`, not
        // `bambu_lab_bambu_pla_basic`.
        let bare = short
            .strip_prefix(vendor.split_whitespace().next().unwrap_or(vendor))
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .unwrap_or(&short);
        if !seen.insert((kind.clone(), short.clone())) {
            continue;
        }
        let mut settings = HashMap::new();
        if let Some(temp) = values
            .get("nozzle_temperature")
            .and_then(first_number)
            .or_else(|| {
                values
                    .get("nozzle_temperature_initial_layer")
                    .and_then(first_number)
            })
        {
            settings.insert("material_print_temperature".to_string(), temp);
        }
        if let Some(bed) = values
            .get("hot_plate_temp")
            .and_then(first_number)
            .or_else(|| values.get("textured_plate_temp").and_then(first_number))
            .or_else(|| values.get("bed_temperature").and_then(first_number))
        {
            settings.insert("material_bed_temperature".to_string(), bed);
        }
        if let Some(density) = values.get("filament_density").and_then(first_number) {
            settings.insert("material_density".to_string(), density);
        }
        // The machine's own start sequence purges and calibrates at this rate,
        // so it has to travel with the filament.
        if let Some(speed) = values
            .get("filament_max_volumetric_speed")
            .and_then(first_number)
        {
            settings.insert("filament_max_volumetric_speed".to_string(), speed);
        }
        let color = text(values.get("default_filament_colour"))
            .filter(|value| value.starts_with('#') && value.len() == 7)
            .unwrap_or_else(|| material_color(&kind).to_string());
        out.push(Material {
            id: format!("{}_{}", slug(vendor), slug(bare)),
            brand: vendor.to_string(),
            name: short,
            color,
            diameter: 1.75,
            filament_type: Some(kind.clone()),
            settings,
            machines: vec![MachineMaterial {
                products: vec![printer.to_string(), model.to_string()],
                ..Default::default()
            }],
        });
    }
    out.sort_by_key(|material| (material_priority(material), material.name.clone()));
    out.truncate(MAX_MATERIALS);
    out
}

fn material_priority(material: &Material) -> u8 {
    let name = material.name.to_ascii_uppercase();
    if name.contains("PLA") {
        0
    } else if name.contains("PETG") {
        1
    } else if name.contains("ABS") {
        2
    } else if name.contains("ASA") {
        3
    } else if name.contains("TPU") {
        4
    } else {
        5
    }
}

fn material_color(kind: &str) -> &'static str {
    match kind.to_ascii_uppercase().as_str() {
        "PLA" => "#d9dde1",
        "PETG" => "#c6dcec",
        "ABS" => "#e8dcc8",
        "ASA" => "#dfe3c8",
        "TPU" => "#d8cbe8",
        _ => "#9aa0a6",
    }
}

/// A profile applies to a printer when its `compatible_printers` names one of
/// the machine files in the group. Libraries that omit the key are treated as
/// universal so a dropdown is never empty.
fn applies_to(values: &Json, machine_names: &[String]) -> bool {
    let Some(list) = values
        .get("compatible_printers")
        .and_then(|value| value.as_array())
    else {
        return true;
    };
    if list.is_empty() {
        return true;
    }
    list.iter().filter_map(|item| item.as_str()).any(|name| {
        machine_names
            .iter()
            .any(|machine| machine.eq_ignore_ascii_case(name))
    })
}

fn cura_infill(pattern: &str) -> String {
    match pattern.to_ascii_lowercase().as_str() {
        "grid" => "grid",
        "gyroid" => "gyroid",
        "cubic" => "cubic",
        "line" | "lines" => "lines",
        "honeycomb" => "honeycomb",
        "lightning" => "lightning",
        "concentric" => "concentric",
        "zig-zag" | "zigzag" => "zigzag",
        "cross" | "cross-hatch" => "cross",
        "triangles" => "triangles",
        _ => "grid",
    }
    .to_string()
}

fn pretty_quality(stem: &str) -> String {
    let head = stem.split(" @").next().unwrap_or(stem);
    // "0.20mm Standard" -> "0.20 mm Standard"
    match head.find("mm") {
        Some(index)
            if index > 0
                && head[..index]
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ch == '.') =>
        {
            format!("{} mm{}", &head[..index], &head[index + 2..])
        }
        _ => head.to_string(),
    }
}

fn polygon_extent(value: &Json) -> Option<(f64, f64, bool)> {
    let points: Vec<(f64, f64)> = value
        .as_array()?
        .iter()
        .filter_map(|point| {
            let text = point.as_str()?;
            let (x, y) = text.split_once('x')?;
            Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
        })
        .collect();
    if points.len() < 3 {
        return None;
    }
    let min_x = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let max_x = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let min_y = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let max_y = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let centered = min_x < -0.01 || min_y < -0.01;
    Some((max_x - min_x, max_y - min_y, centered))
}

fn number_value(value: &Json) -> Option<f64> {
    match value {
        Json::Number(number) => number.as_f64(),
        Json::String(text) => text.trim().trim_end_matches('%').parse().ok(),
        Json::Array(items) => items.first().and_then(number_value),
        _ => None,
    }
}

fn first_number(value: &Json) -> Option<f64> {
    match value {
        Json::Array(items) => items.iter().find_map(number_value),
        other => number_value(other),
    }
}

fn percent_value(value: &Json) -> Option<f64> {
    number_value(value)
}

fn truthy(value: &Json) -> Option<bool> {
    match value {
        Json::Bool(flag) => Some(*flag),
        Json::Number(number) => Some(number.as_f64().unwrap_or(0.0) != 0.0),
        Json::String(text) => match text.trim() {
            "1" | "true" | "True" => Some(true),
            "0" | "false" | "False" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn text(value: Option<&Json>) -> Option<String> {
    match value? {
        Json::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Json::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn first_text(value: Option<&Json>) -> Option<String> {
    match value? {
        Json::Array(items) => items.iter().find_map(|item| text(Some(item))),
        other => text(Some(other)),
    }
}

fn number(text: &str) -> f64 {
    text.trim().parse().unwrap_or(0.0)
}

/// Lowercase ASCII identifier: `Bambu Lab A1 0.4 nozzle` -> `bambu_lab_a1_0_4_nozzle`.
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_separator = true;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_separator = false;
        } else if !last_separator {
            out.push('_');
            last_separator = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed() -> Option<Library> {
        let root = profiles_root()?;
        let library = Library::from_root(Some(root));
        (!library.is_empty()).then_some(library)
    }

    #[test]
    fn a_missing_library_is_empty_and_silent() {
        let library = Library::from_root(Some(PathBuf::from("C:/definitely/not/here")));
        assert!(library.is_empty());
        assert!(library.definitions.is_empty());
    }

    #[test]
    fn reads_bambu_printers_with_real_beds() {
        let Some(library) = installed() else {
            return;
        };
        let printers: Vec<&Definition> = library
            .definitions
            .iter()
            .filter(|definition| !definition.is_extruder)
            .collect();
        assert!(printers.len() >= 10, "found {} printers", printers.len());
        let x1 = printers
            .iter()
            .find(|definition| definition.name.contains("X1 Carbon"))
            .expect("X1 Carbon profile");
        let width = x1.settings["machine_width"].default_value.clone().unwrap();
        let depth = x1.settings["machine_depth"].default_value.clone().unwrap();
        assert_eq!(width.as_f64().unwrap(), 256.0);
        assert_eq!(depth.as_f64().unwrap(), 256.0);
        // Machines carry the short name and let the window print the vendor
        // next to it, so pick the A1 by id rather than by display name.
        let a1 = printers
            .iter()
            .find(|definition| definition.id == "bambu_lab_a1")
            .expect("A1 profile");
        assert_eq!(a1.name, "A1");
        assert_eq!(
            a1.settings["machine_depth"]
                .default_value
                .clone()
                .unwrap()
                .as_f64(),
            Some(256.0)
        );
    }

    #[test]
    fn converts_filament_temperatures() {
        let Some(library) = installed() else {
            return;
        };
        let pla = library
            .materials
            .iter()
            .find(|material| material.name.contains("PLA Basic"))
            .expect("PLA Basic");
        let temp = pla.settings["material_print_temperature"];
        assert!(temp > 150.0 && temp < 320.0, "nozzle {temp}");
        let bed = pla.settings["material_bed_temperature"];
        assert!(bed >= 35.0, "bed {bed}");
        assert_eq!(pla.diameter, 1.75);
    }

    #[test]
    fn converts_process_overrides() {
        let Some(library) = installed() else {
            return;
        };
        let quality = library
            .instances
            .iter()
            .find(|instance| {
                instance.type_name == "quality" && instance.name.contains("0.20 mm Standard")
            })
            .expect("0.20 mm Standard process");
        let height: f64 = quality.values["layer_height"].parse().unwrap();
        assert!((height - 0.2).abs() < 1e-9);
        assert!(quality.values.contains_key("infill_sparse_density"));
        assert!(quality.values.contains_key("wall_line_count"));
        assert_eq!(quality.values.get("speed_wall_0").map(String::as_str), Some("200"));
        assert_eq!(quality.values.get("speed_travel").map(String::as_str), Some("700"));
    }

    #[test]
    fn the_whole_library_reads_quickly() {
        let Some(root) = profiles_root() else { return };
        // The first read is the disk's time, not the code's: on a cold page
        // cache Windows Defender opens every one of the ~780 profile files and
        // that alone took 38 s on this machine. What this test guards is that
        // the library is parsed once per printer rather than once per model, so
        // it warms the cache and then times a second read.
        let _ = Library::from_root(Some(root.clone()));
        // Best of three. The whole workspace suite runs its tests in parallel
        // and some of them start CuraEngine, so a single read can be descheduled
        // for seconds while the code is fine; a read that is slow *by itself*
        // stays slow in all three. The bug this guards made every read slow, so
        // the minimum still catches it.
        let mut library = None;
        let mut best = std::time::Duration::MAX;
        for _ in 0..3 {
            let start = std::time::Instant::now();
            let read = Library::from_root(Some(root.clone()));
            best = best.min(start.elapsed());
            library = Some(read);
        }
        let library = library.expect("three reads");
        assert!(!library.is_empty());
        println!(
            "library: {} definitions, {} instances, {} materials, best of three in {:?}",
            library.definitions.len(),
            library.instances.len(),
            library.materials.len(),
            best
        );
        // The whole library is re-read on every launch, so this has to stay
        // cheap: it used to take eight seconds because both profile folders
        // were parsed once per printer model. A repeat of that bug blows
        // straight through this bound even with the cache warm.
        assert!(
            best.as_millis() < 5000,
            "reading the profile library took {best:?} at best"
        );
    }

    #[test]
    fn slugs_are_stable() {
        assert_eq!(slug("Bambu Lab A1 0.4 nozzle"), "bambu_lab_a1_0_4_nozzle");
        assert_eq!(slug("0.20mm Standard @BBL A1"), "0_20mm_standard_bbl_a1");
        assert_eq!(slug("--PLA--"), "pla");
    }
}

/// Drop the engine's heat prelude from a file that carries a machine's own
/// start sequence.
///
/// CuraEngine emits `M140`/`M190`/`M104`/`M109` before the start G-code it was
/// given, whatever that G-code says. On a Bambu machine those lines are both
/// wrong and in the wrong place: the A1's own sequence heats to the standby
/// temperature, runs its macros, levels and then heats to print temperature,
/// and a file that has already commanded 220 °C before the machine's macro does
/// not describe the same print. The engine's numbers come from the same profile
/// as the template's, so dropping the prelude loses nothing.
///
/// Only the lines before the first `;===== machine:` are touched. Everything
/// after it is the machine's sequence and the print.
pub fn strip_engine_prelude(gcode: &str) -> String {
    const HEAT: [&str; 5] = ["M104", "M109", "M140", "M190", "M105"];
    let Some(banner) = gcode
        .lines()
        .position(|line| line.trim_start().starts_with(";===== machine:"))
    else {
        return gcode.to_owned();
    };
    let mut kept: Vec<&str> = Vec::with_capacity(banner);
    for line in gcode.lines().take(banner) {
        let trimmed = line.trim_start();
        let is_heat = HEAT.iter().any(|code| {
            trimmed.strip_prefix(code).is_some_and(|rest| {
                rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')
            })
        });
        if !is_heat {
            kept.push(line);
        }
    }
    let mut text = kept.join("\n");
    text.push('\n');
    text.push_str(&gcode.lines().skip(banner).collect::<Vec<_>>().join("\n"));
    if gcode.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Replace the bounds CuraEngine puts at the top of a file.
///
/// The engine writes `;MINX:2.14748e+06` and `;MAXX:-2.14748e+06` — `f32::MAX`
/// and its negation — when it is handed bare STL files instead of a scene it
/// built itself, which is how Shale calls it. The numbers claim a bounding box
/// two million millimetres wide, in a file whose every move is inside the plate.
/// Cura's own header describes the scene's bounds in the coordinates the moves
/// use, and Shale knows those exactly: the baked, engine-space meshes it just
/// wrote out. So the six lines are rewritten from that union and nothing else in
/// the file is touched.
///
/// A file without those lines is returned unchanged: a generic Marlin header
/// has no bounds to correct.
pub fn rewrite_header_bounds(gcode: &str, min: [f64; 3], max: [f64; 3]) -> String {
    const KEYS: [(&str, usize, bool); 6] = [
        (";MINX:", 0, false),
        (";MINY:", 1, false),
        (";MINZ:", 2, false),
        (";MAXX:", 0, true),
        (";MAXY:", 1, true),
        (";MAXZ:", 2, true),
    ];
    // A bounding box of infinities would mean no model was measured, and
    // writing that into a file is no better than leaving the engine's sentinel.
    if min.iter().chain(max.iter()).any(|value| !value.is_finite()) {
        return gcode.to_string();
    }
    let mut out = String::with_capacity(gcode.len());
    let mut done = [false; 6];
    for line in gcode.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let mut replaced = None;
        for (slot, (key, axis, upper)) in KEYS.iter().enumerate() {
            if done[slot] || bare.len() <= key.len() || !bare.starts_with(key) {
                continue;
            }
            let value = if *upper { max[*axis] } else { min[*axis] };
            replaced = Some(format!("{key}{}", trim(value)));
            done[slot] = true;
            break;
        }
        match replaced {
            Some(text) => {
                out.push_str(&text);
                out.push_str(&line[bare.len()..]);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Three decimals, trailing zeros gone: `90.5`, not `90.500`, and not the
/// exponent Cura prints when the number it holds is a sentinel.
fn trim(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod header_bounds_tests {
    use super::rewrite_header_bounds;

    #[test]
    fn the_engines_sentinel_is_replaced_with_the_real_box() {
        let file = concat!(
            ";FLAVOR:Marlin\n",
            ";MINX:2.14748e+06\n",
            ";MINY:2.14748e+06\n",
            ";MINZ:2.14748e+06\n",
            ";MAXX:-2.14748e+06\n",
            ";MAXY:-2.14748e+06\n",
            ";MAXZ:-2.14748e+06\n",
            ";LAYER_COUNT:54\n",
            "G28\n",
        );
        let out = rewrite_header_bounds(file, [-20.0, -15.5, 0.2], [20.0, 15.5, 12.4]);
        assert!(out.contains(";MINX:-20\n"), "{out}");
        assert!(out.contains(";MINY:-15.5\n"), "{out}");
        assert!(out.contains(";MINZ:0.2\n"), "{out}");
        assert!(out.contains(";MAXX:20\n"), "{out}");
        assert!(out.contains(";MAXY:15.5\n"), "{out}");
        assert!(out.contains(";MAXZ:12.4\n"), "{out}");
        assert!(out.ends_with("G28\n"), "the moves are untouched");
        assert_eq!(out.lines().count(), file.lines().count());
    }

    #[test]
    fn a_header_without_bounds_is_left_alone() {
        let file = ";FLAVOR:Marlin\nG28\n";
        assert_eq!(rewrite_header_bounds(file, [0.0; 3], [1.0; 3]), file);
    }

    #[test]
    fn an_unmeasured_box_does_not_overwrite_a_file() {
        let file = ";MINX:1\n";
        let out = rewrite_header_bounds(file, [f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        assert_eq!(out, file);
    }
}

#[cfg(test)]
mod prelude_tests {
    use super::strip_engine_prelude;

    #[test]
    fn the_engines_heat_is_dropped_and_the_machine_sequence_is_not() {
        let file = concat!(
            ";FLAVOR:Marlin\n",
            ";TARGET_MACHINE.NAME:Bambu Lab A1\n",
            "M140 S65\n",
            "M105\n",
            "M190 S65\n",
            "M104 S220\n",
            "M109 S220\n",
            ";===== machine: A1 =========================\n",
            "M104 S140\n",
            "M140 S65\n",
            "M109 S220\n",
        );
        let out = strip_engine_prelude(file);
        assert_eq!(
            out,
            concat!(
                ";FLAVOR:Marlin\n",
                ";TARGET_MACHINE.NAME:Bambu Lab A1\n",
                ";===== machine: A1 =========================\n",
                "M104 S140\n",
                "M140 S65\n",
                "M109 S220\n",
            ),
            "only the prelude went"
        );
    }

    #[test]
    fn a_file_without_a_machine_sequence_is_left_alone() {
        let file = "M140 S60\nM104 S210\nG28\n";
        assert_eq!(strip_engine_prelude(file), file);
    }
}
