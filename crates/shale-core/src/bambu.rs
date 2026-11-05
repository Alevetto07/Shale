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

use serde_json::Value as Json;

use crate::definition::{Definition, RawSetting};
use crate::instance::InstanceFile;
use crate::material::{MachineMaterial, Material};

/// Vendors read by default. Bambu Studio's own tree is the one that is
/// installed on a machine that slices for a Bambu printer; the others share the
/// schema and can be switched on with `SHALE_ORCA_VENDORS`.
const DEFAULT_VENDORS: &str = "BBL";

/// More nozzles than this per model is noise in a dropdown.
const MAX_VARIANTS: usize = 8;
/// Quality profiles shown per printer, best-known first.
const MAX_QUALITIES: usize = 14;
/// Filaments shown per printer.
const MAX_MATERIALS: usize = 40;
/// Depth guard for `inherits` chains; the deepest real chain is 3.
const MAX_DEPTH: usize = 12;

pub struct Library {
    pub definitions: Vec<Definition>,
    pub instances: Vec<InstanceFile>,
    pub materials: Vec<Material>,
}

impl Library {
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    /// Load from the configured profile root, or an empty library when no
    /// slicer is installed. Never fails: a missing library is normal.
    pub fn load() -> Self {
        Self::from_root(root())
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
            let mut nozzles: Vec<String> = group.iter().map(|profile| profile.nozzle.clone()).collect();
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
            self.instances
                .extend(quality_instances(&processes, &id, &names, &base.default_quality));
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

fn root() -> Option<PathBuf> {
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

fn quality_instances(
    processes: &[(String, Json)],
    printer: &str,
    machine_names: &[String],
    default_quality: &str,
) -> Vec<InstanceFile> {
    let mut out = Vec::new();
    for (stem, values) in processes {
        if !applies_to(&values, machine_names) {
            continue;
        }
        let Some(layer_height) = values.get("layer_height").and_then(number_value) else {
            continue;
        };
        let mut settings = HashMap::new();
        settings.insert("layer_height".to_string(), format!("{layer_height}"));
        if let Some(walls) = values.get("wall_loops").and_then(number_value) {
            settings.insert("wall_line_count".to_string(), format!("{}", walls.round()));
        }
        if let Some(infill) = values.get("sparse_infill_density").and_then(percent_value) {
            settings.insert("infill_sparse_density".to_string(), format!("{infill}"));
        }
        if let Some(pattern) = text(values.get("sparse_infill_pattern")) {
            settings.insert("infill_pattern".to_string(), cura_infill(&pattern));
        }
        if let Some(layers) = values.get("top_shell_layers").and_then(number_value) {
            settings.insert("top_layers".to_string(), format!("{}", layers.round()));
        }
        if let Some(layers) = values.get("bottom_shell_layers").and_then(number_value) {
            settings.insert(
                "bottom_layers".to_string(),
                format!("{}", layers.round()),
            );
        }
        if let Some(support) = values.get("enable_support").and_then(truthy) {
            settings.insert(
                "support_enable".to_string(),
                if support { "True" } else { "False" }.to_string(),
            );
        }
        let name = pretty_quality(&stem);
        out.push(InstanceFile {
            name,
            definition: printer.to_string(),
            type_name: "quality".to_string(),
            quality_type: Some(slug(&stem)),
            variant: None,
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
    out.truncate(MAX_QUALITIES);
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
            .or_else(|| values.get("nozzle_temperature_initial_layer").and_then(first_number))
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
        let color = text(values.get("default_filament_colour"))
            .filter(|value| value.starts_with('#') && value.len() == 7)
            .unwrap_or_else(|| material_color(&kind).to_string());
        out.push(Material {
            id: format!("{}_{}", slug(vendor), slug(bare)),
            brand: vendor.to_string(),
            name: short,
            color,
            diameter: 1.75,
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
    let Some(list) = values.get("compatible_printers").and_then(|value| value.as_array()) else {
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
        Some(index) if index > 0 && head[..index].chars().all(|ch| ch.is_ascii_digit() || ch == '.') => {
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
        let root = root()?;
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
            a1.settings["machine_depth"].default_value.clone().unwrap().as_f64(),
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
            .find(|instance| instance.type_name == "quality" && instance.name.contains("0.20 mm Standard"))
            .expect("0.20 mm Standard process");
        let height: f64 = quality.values["layer_height"].parse().unwrap();
        assert!((height - 0.2).abs() < 1e-9);
        assert!(quality.values.contains_key("infill_sparse_density"));
        assert!(quality.values.contains_key("wall_line_count"));
    }

    #[test]
    fn the_whole_library_reads_quickly() {
        let Some(root) = root() else { return };
        // The first read is the disk's time, not the code's: on a cold page
        // cache Windows Defender opens every one of the ~780 profile files and
        // that alone took 38 s on this machine. What this test guards is that
        // the library is parsed once per printer rather than once per model, so
        // it warms the cache and then times a second read.
        let _ = Library::from_root(Some(root.clone()));
        let start = std::time::Instant::now();
        let library = Library::from_root(Some(root));
        let elapsed = start.elapsed();
        assert!(!library.is_empty());
        println!(
            "library: {} definitions, {} instances, {} materials in {:?}",
            library.definitions.len(),
            library.instances.len(),
            library.materials.len(),
            elapsed
        );
        // The whole library is re-read on every launch, so this has to stay
        // cheap: it used to take eight seconds because both profile folders
        // were parsed once per printer model. A repeat of that bug blows
        // straight through this bound even with the cache warm.
        assert!(
            elapsed.as_millis() < 5000,
            "reading the profile library took {elapsed:?}"
        );
    }

    #[test]
    fn slugs_are_stable() {
        assert_eq!(slug("Bambu Lab A1 0.4 nozzle"), "bambu_lab_a1_0_4_nozzle");
        assert_eq!(slug("0.20mm Standard @BBL A1"), "0_20mm_standard_bbl_a1");
        assert_eq!(slug("--PLA--"), "pla");
    }
}
