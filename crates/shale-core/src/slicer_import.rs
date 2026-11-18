//! Detect other slicers, list user presets, and import them into Shale storage.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::bambu::{flatten_preset_from_dirs, profiles_root};
use crate::preset::{self, Preset};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlicerInstall {
    pub id: String,
    pub label: String,
    pub found: bool,
    pub config_root: Option<String>,
    pub printers: u32,
    pub filaments: u32,
    pub processes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresetOffer {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub name: String,
    pub slicer: String,
    pub supported: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportLine {
    pub path: String,
    pub name: String,
    pub outcome: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SlicerImportReport {
    pub imported: u32,
    pub skipped: u32,
    pub unsupported: u32,
    pub lines: Vec<ImportLine>,
}

const PRINTER_DIRS: &[&str] = &["machine", "printer", "printers", "snapshots"];
const FILAMENT_DIRS: &[&str] = &["filament", "filaments", "material"];
const PROCESS_DIRS: &[&str] = &["process", "processes", "print", "quality", "definition"];

fn candidate_paths() -> Vec<(String, String, Vec<PathBuf>)> {
    let mut bambu = Vec::new();
    let mut orca = Vec::new();
    let mut prusa = Vec::new();
    let mut cura = Vec::new();

    if let Some(appdata) = std::env::var_os("APPDATA") {
        let appdata = PathBuf::from(appdata);
        bambu.push(appdata.join("BambuStudio"));
        orca.push(appdata.join("OrcaSlicer"));
        prusa.push(appdata.join("PrusaSlicer"));
        cura.push(appdata.join("cura"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        cura.push(PathBuf::from(local).join("cura"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        bambu.push(home.join(".config/BambuStudio"));
        bambu.push(home.join("Library/Application Support/BambuStudio"));
        orca.push(home.join(".config/OrcaSlicer"));
        prusa.push(home.join(".config/PrusaSlicer"));
        cura.push(home.join(".config/cura"));
    }
    vec![
        ("bambu".into(), "Bambu Studio".into(), bambu),
        ("orca".into(), "OrcaSlicer".into(), orca),
        ("prusa".into(), "PrusaSlicer".into(), prusa),
        ("cura".into(), "UltiMaker Cura".into(), cura),
    ]
}

fn resolve_config_root(slicer_id: &str) -> Option<PathBuf> {
    candidate_paths()
        .into_iter()
        .find(|(id, _, _)| id == slicer_id)
        .and_then(|(_, _, paths)| paths.into_iter().find(|p| p.is_dir()))
}

/// Directory for imported Orca-family JSON trees (host sets `SHALE_IMPORTED_PROFILES`).
pub fn imported_profiles_tree(dest: &Path) -> PathBuf {
    dest.join("imported-profiles")
}

pub fn detect_installs() -> Vec<SlicerInstall> {
    let mut out = Vec::new();
    for (id, label, paths) in candidate_paths() {
        if let Some(root) = paths.into_iter().find(|path| path.is_dir()) {
            let (printers, filaments, processes) = summarise_tree(&root);
            out.push(SlicerInstall {
                id,
                label,
                found: true,
                config_root: Some(root.display().to_string()),
                printers,
                filaments,
                processes,
            });
        } else {
            out.push(SlicerInstall {
                id,
                label,
                found: false,
                config_root: None,
                printers: 0,
                filaments: 0,
                processes: 0,
            });
        }
    }
    out
}

fn summarise_tree(root: &Path) -> (u32, u32, u32) {
    let mut printers = 0u32;
    let mut filaments = 0u32;
    let mut processes = 0u32;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if stack.len() < 16 {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let parent = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if dir_matches(&parent, PRINTER_DIRS) || dir_matches(&name, PRINTER_DIRS) {
                printers += 1;
            } else if dir_matches(&parent, FILAMENT_DIRS) || dir_matches(&name, FILAMENT_DIRS) {
                filaments += 1;
            } else if dir_matches(&parent, PROCESS_DIRS) || dir_matches(&name, PROCESS_DIRS) {
                processes += 1;
            }
        }
    }
    (printers, filaments, processes)
}

fn dir_matches(name: &str, allowed: &[&str]) -> bool {
    let lower = name.to_lowercase();
    allowed.iter().any(|entry| lower.contains(entry))
}

fn bundled_vendor_dirs() -> Vec<PathBuf> {
    let Some(root) = profiles_root() else {
        return Vec::new();
    };
    ["BBL", "Creality"]
        .iter()
        .map(|code| root.join(code))
        .filter(|path| path.is_dir())
        .collect()
}

fn orca_family_dirs(system: &[PathBuf], user_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = system.to_vec();
    for root in user_roots {
        for kind in ["machine", "filament", "process"] {
            let path = root.join(kind);
            if path.is_dir() {
                dirs.push(path);
            }
        }
        // Bambu user layout: user/<id>/{machine,filament,process}
        if let Ok(read) = std::fs::read_dir(root.join("user")) {
            for entry in read.flatten() {
                let user = entry.path();
                for kind in ["machine", "filament", "process"] {
                    let path = user.join(kind);
                    if path.is_dir() {
                        dirs.push(path);
                    }
                }
            }
        }
    }
    dirs
}

fn discover_user_json(config_root: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut stack = vec![config_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if stack.len() < 24 {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let parent = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let kind = if dir_matches(&parent, PRINTER_DIRS) {
                "machine"
            } else if dir_matches(&parent, FILAMENT_DIRS) {
                "filament"
            } else if dir_matches(&parent, PROCESS_DIRS) {
                "process"
            } else {
                continue;
            };
            found.push((kind.to_string(), path));
        }
    }
    found
}

fn display_name_from_json(path: &Path, kind: &str) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("preset")
        .to_string();
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(value) = serde_json::from_str::<Json>(&text) {
            if let Some(name) = value.get("name").and_then(|v| v.as_str()) {
                if !name.trim().is_empty() {
                    return format!("{name} ({kind})");
                }
            }
        }
    }
    format!("{stem} ({kind})")
}

pub fn list_preset_offers(slicer_id: &str) -> Result<Vec<PresetOffer>, String> {
    let Some(config_root) = resolve_config_root(slicer_id) else {
        return Ok(Vec::new());
    };
    let mut offers = Vec::new();
    match slicer_id {
        "bambu" | "orca" => {
            let bundled = bundled_vendor_dirs();
            let user_dirs = vec![config_root.clone()];
            for (kind, path) in discover_user_json(&config_root) {
                let lookup = orca_family_lookup_dirs(&bundled, &user_dirs, &kind);
                let (supported, note) = match flatten_preset_from_dirs(&path, &lookup) {
                    Ok(_) => (true, None),
                    Err(err) => (false, Some(err)),
                };
                let id = format!("{slicer_id}:{kind}:{}", path.display());
                offers.push(PresetOffer {
                    id,
                    path: path.display().to_string(),
                    kind,
                    name: display_name_from_json(&path, &kind),
                    slicer: slicer_id.to_string(),
                    supported,
                    note,
                });
            }
        }
        "prusa" => {
            for (kind, path) in discover_prusa_presets(&config_root) {
                let (supported, note) = match preview_prusa_ini(&path) {
                    Ok(map) if !map.is_empty() => (true, None),
                    Ok(_) => (
                        false,
                        Some("no mappable slice settings in this preset".into()),
                    ),
                    Err(err) => (false, Some(err)),
                };
                offers.push(PresetOffer {
                    id: format!("prusa:{kind}:{}", path.display()),
                    path: path.display().to_string(),
                    kind,
                    name: path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("preset")
                        .to_string(),
                    slicer: "prusa".into(),
                    supported,
                    note,
                });
            }
        }
        "cura" => {
            for (kind, path) in discover_cura_presets(&config_root) {
                let (supported, note) = match preview_cura_cfg(&path) {
                    Ok(map) if !map.is_empty() => (true, None),
                    Ok(_) => (
                        false,
                        Some("no mappable slice settings in this profile".into()),
                    ),
                    Err(err) => (false, Some(err)),
                };
                offers.push(PresetOffer {
                    id: format!("cura:{kind}:{}", path.display()),
                    path: path.display().to_string(),
                    kind,
                    name: path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("preset")
                        .to_string(),
                    slicer: "cura".into(),
                    supported,
                    note,
                });
            }
        }
        _ => return Err(format!("unknown slicer id `{slicer_id}`")),
    }
    offers.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(offers)
}

fn orca_family_lookup_dirs(bundled: &[PathBuf], user_roots: &[PathBuf], kind: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = bundled
        .iter()
        .map(|vendor| vendor.join(kind))
        .filter(|p| p.is_dir())
        .collect();
    for root in user_roots {
        let direct = root.join(kind);
        if direct.is_dir() {
            dirs.push(direct);
        }
        let user = root.join("user");
        if user.is_dir() {
            if let Ok(read) = std::fs::read_dir(user) {
                for entry in read.flatten() {
                    let nested = entry.path().join(kind);
                    if nested.is_dir() {
                        dirs.push(nested);
                    }
                }
            }
        }
    }
    dirs
}

fn safe_file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("preset")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect()
}

fn write_merged_json(target: &Path, merged: &Json) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut flat = merged.clone();
    if let Json::Object(map) = &mut flat {
        map.remove("inherits");
    }
    let text = serde_json::to_string_pretty(&flat).map_err(|e| e.to_string())?;
    std::fs::write(target, text).map_err(|e| e.to_string())
}

pub fn import_presets(
    slicer_id: &str,
    selected_paths: &[String],
    imported_tree: &Path,
    shale_preset_dir: &Path,
) -> Result<SlicerImportReport, String> {
    let Some(config_root) = resolve_config_root(slicer_id) else {
        return Err(format!("`{slicer_id}` is not installed on this machine"));
    };
    let bundled = bundled_vendor_dirs();
    let user_dirs = vec![config_root];
    let mut report = SlicerImportReport::default();

    for path_str in selected_paths {
        let path = PathBuf::from(path_str);
        if !path.is_file() {
            report.skipped += 1;
            report.lines.push(ImportLine {
                path: path_str.clone(),
                name: path_str.clone(),
                outcome: "skipped".into(),
                detail: Some("file not found".into()),
            });
            continue;
        }
        match slicer_id {
            "bambu" | "orca" => {
                let kind = infer_json_kind(&path).unwrap_or_else(|| "process".to_string());
                let lookup = orca_family_lookup_dirs(&bundled, &user_dirs, &kind);
                match flatten_preset_from_dirs(&path, &lookup) {
                    Ok(merged) => {
                        let stem = safe_file_stem(&path);
                        let dest = imported_tree
                            .join("BBL")
                            .join(&kind)
                            .join(format!("imported_{stem}.json"));
                        if let Err(err) = write_merged_json(&dest, &merged) {
                            report.skipped += 1;
                            report.lines.push(ImportLine {
                                path: path_str.clone(),
                                name: display_name_from_json(&path, &kind),
                                outcome: "skipped".into(),
                                detail: Some(err),
                            });
                            continue;
                        }
                        report.imported += 1;
                        report.lines.push(ImportLine {
                            path: path_str.clone(),
                            name: display_name_from_json(&path, &kind),
                            outcome: "imported".into(),
                            detail: Some(dest.display().to_string()),
                        });
                    }
                    Err(err) => {
                        report.skipped += 1;
                        report.lines.push(ImportLine {
                            path: path_str.clone(),
                            name: path.display().to_string(),
                            outcome: "skipped".into(),
                            detail: Some(err),
                        });
                    }
                }
            }
            "prusa" => match map_prusa_ini(&path) {
                Ok(values) => match save_process_preset(shale_preset_dir, &path, values) {
                    Ok(name) => {
                        report.imported += 1;
                        report.lines.push(ImportLine {
                            path: path_str.clone(),
                            name: name.clone(),
                            outcome: "imported".into(),
                            detail: Some(format!("Shale process preset `{name}`")),
                        });
                    }
                    Err(err) => {
                        report.skipped += 1;
                        report.lines.push(ImportLine {
                            path: path_str.clone(),
                            name: path.display().to_string(),
                            outcome: "skipped".into(),
                            detail: Some(err),
                        });
                    }
                },
                Err(err) => {
                    report.unsupported += 1;
                    report.lines.push(ImportLine {
                        path: path_str.clone(),
                        name: path.display().to_string(),
                        outcome: "unsupported".into(),
                        detail: Some(err),
                    });
                }
            },
            "cura" => match map_cura_cfg(&path) {
                Ok(values) => match save_process_preset(shale_preset_dir, &path, values) {
                    Ok(name) => {
                        report.imported += 1;
                        report.lines.push(ImportLine {
                            path: path_str.clone(),
                            name: name.clone(),
                            outcome: "imported".into(),
                            detail: Some(format!("Shale process preset `{name}`")),
                        });
                    }
                    Err(err) => {
                        report.skipped += 1;
                        report.lines.push(ImportLine {
                            path: path_str.clone(),
                            name: path.display().to_string(),
                            outcome: "skipped".into(),
                            detail: Some(err),
                        });
                    }
                },
                Err(err) => {
                    report.unsupported += 1;
                    report.lines.push(ImportLine {
                        path: path_str.clone(),
                        name: path.display().to_string(),
                        outcome: "unsupported".into(),
                        detail: Some(err),
                    });
                }
            },
            _ => {
                report.unsupported += 1;
                report.lines.push(ImportLine {
                    path: path_str.clone(),
                    name: path.display().to_string(),
                    outcome: "unsupported".into(),
                    detail: Some("unknown slicer id".into()),
                });
            }
        }
    }

    Ok(report)
}

fn infer_json_kind(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    let name = parent.file_name()?.to_string_lossy().to_lowercase();
    if dir_matches(&name, PRINTER_DIRS) {
        return Some("machine".into());
    }
    if dir_matches(&name, FILAMENT_DIRS) {
        return Some("filament".into());
    }
    if dir_matches(&name, PROCESS_DIRS) {
        return Some("process".into());
    }
    None
}

const KNOWN_PRESET_KEYS: &[&str] = &[
    "layer_height",
    "wall_line_count",
    "infill_sparse_density",
    "material_print_temperature",
    "material_bed_temperature",
    "speed_print",
    "retraction_amount",
    "retraction_speed",
];

fn save_process_preset(
    dir: &Path,
    source: &Path,
    values: BTreeMap<String, String>,
) -> Result<String, String> {
    let stem = safe_file_stem(source);
    let name = format!("Imported {stem}");
    let preset = Preset::new(&name, values).map_err(|e| e.to_string())?;
    preset.check(KNOWN_PRESET_KEYS).map_err(|e| e.to_string())?;
    preset::save(dir, &preset).map_err(|e| e.to_string())?;
    Ok(name)
}

fn discover_prusa_presets(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if stack.len() < 12 {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) == Some("ini") {
                out.push(("process".into(), path));
            }
        }
    }
    out
}

fn preview_prusa_ini(path: &Path) -> Result<BTreeMap<String, String>, String> {
    map_prusa_ini(path)
}

fn map_prusa_ini(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut section = String::new();
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with(';') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(['[', ']'].as_ref()).to_string();
            continue;
        }
        if !section.starts_with("print") && section != "filament" {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let raw = raw.trim();
        if let Some(mapped) = map_prusa_key(key, raw) {
            values.insert(mapped.0, mapped.1);
        }
    }
    Ok(values)
}

fn map_prusa_key(key: &str, raw: &str) -> Option<(String, String)> {
    match key {
        "layer_height" => Some(("layer_height".into(), raw.into())),
        "perimeters" | "external_perimeters" => Some(("wall_line_count".into(), raw.into())),
        "fill_density" => {
            let value = raw.trim_end_matches('%').trim();
            Some(("infill_sparse_density".into(), value.into()))
        }
        "temperature" => Some(("material_print_temperature".into(), raw.into())),
        "bed_temperature" => Some(("material_bed_temperature".into(), raw.into())),
        "perimeter_speed" | "infill_speed" | "travel_speed" => {
            Some(("speed_print".into(), raw.into()))
        }
        "retract_length" => Some(("retraction_amount".into(), raw.into())),
        "retract_speed" => Some(("retraction_speed".into(), raw.into())),
        _ => None,
    }
}

fn discover_cura_presets(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if stack.len() < 12 {
                    stack.push(path);
                }
                continue;
            }
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            let stem = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if ext == "cfg" && (stem.contains("quality") || stem.contains("inst")) {
                out.push(("process".into(), path));
            }
        }
    }
    out
}

fn preview_cura_cfg(path: &Path) -> Result<BTreeMap<String, String>, String> {
    map_cura_cfg(path)
}

fn map_cura_cfg(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut in_values = false;
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line == "[values]" {
            in_values = true;
            continue;
        }
        if line.starts_with('[') {
            in_values = false;
            continue;
        }
        if !in_values {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if KNOWN_PRESET_KEYS.contains(&key) {
            values.insert(key.to_string(), raw.trim().to_string());
        }
    }
    Ok(values)
}

/// Parse a small manifest listing preset files (used by tests and the wizard report).
pub fn parse_import_manifest(text: &str) -> Result<Vec<String>, String> {
    let value: Json = serde_json::from_str(text).map_err(|err| format!("manifest json: {err}"))?;
    let items = value
        .get("presets")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "manifest missing presets array".to_string())?;
    let mut names = Vec::new();
    for item in items {
        let name = item
            .as_str()
            .or_else(|| item.get("name").and_then(|n| n.as_str()))
            .ok_or_else(|| "preset entry needs a name".to_string())?;
        names.push(name.trim().to_string());
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bambu::set_test_profiles_root;

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slicer-import")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shale-slicer-import-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn bambu_user_preset_resolves_against_bundled() {
        let bundled = fixture_root().join("bundled");
        set_test_profiles_root(Some(bundled.clone()));
        let user = fixture_root().join("appdata/BambuStudio");
        let preset = user.join("user/default/process/custom.json");
        let lookup = orca_family_lookup_dirs(&[bundled.join("BBL")], &[user], "process");
        let merged = flatten_preset_from_dirs(&preset, &lookup).expect("merge");
        let layer = merged
            .get("layer_height")
            .and_then(|v| v.as_f64())
            .expect("layer_height");
        assert!((layer - 0.24).abs() < 0.001);
        set_test_profiles_root(None);
    }

    #[test]
    fn import_writes_overlay_json() {
        let bundled = fixture_root().join("bundled");
        set_test_profiles_root(Some(bundled.clone()));
        std::env::set_var("SHALE_TEST", "1");
        let appdata = fixture_root().join("appdata");
        std::env::set_var("APPDATA", appdata);
        let dest = scratch("overlay");
        let preset_dir = scratch("presets");
        let preset = appdata.join("BambuStudio/user/default/process/custom.json");
        let report = import_presets(
            "bambu",
            &[preset.display().to_string()],
            &dest,
            &preset_dir,
        )
        .expect("import");
        assert_eq!(report.imported, 1);
        let written = dest.join("BBL/process/imported_custom.json");
        assert!(written.is_file());
        set_test_profiles_root(None);
        let _ = std::fs::remove_dir_all(&dest);
        let _ = std::fs::remove_dir_all(&preset_dir);
    }

    #[test]
    fn prusa_ini_maps_known_keys() {
        let path = fixture_root().join("prusa/custom.ini");
        let map = map_prusa_ini(&path).expect("parse");
        assert_eq!(map.get("layer_height").map(String::as_str), Some("0.22"));
        assert_eq!(map.get("wall_line_count").map(String::as_str), Some("3"));
    }

    #[test]
    fn cura_cfg_maps_values_section() {
        let path = fixture_root().join("cura/draft.inst.cfg");
        let map = map_cura_cfg(&path).expect("parse");
        assert_eq!(map.get("infill_sparse_density").map(String::as_str), Some("20"));
    }

    #[test]
    fn list_offers_marks_missing_inherits() {
        let bundled = fixture_root().join("bundled");
        set_test_profiles_root(Some(bundled));
        let broken = fixture_root().join("broken-user/BambuStudio");
        let preset = broken.join("user/default/process/orphan.json");
        let lookup = orca_family_lookup_dirs(
            &[bundled.join("BBL")],
            &[broken.clone()],
            "process",
        );
        assert!(flatten_preset_from_dirs(&preset, &lookup).is_err());
        set_test_profiles_root(None);
    }
}