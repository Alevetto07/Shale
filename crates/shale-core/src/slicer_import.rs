//! Detect other slicers, list user presets, and import them into Shale storage.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::bambu::{flatten_presets_shared, profiles_root};
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

/// Counts shown while a check or an import is running. `total` is 0 until the
/// walk knows how many files it will look at.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ScanCounts {
    pub seen: u32,
    pub total: u32,
    pub printers: u32,
    pub filaments: u32,
    pub processes: u32,
    pub warnings: u32,
}

/// Compact result of a check. Paths stay here so the import can run without
/// another walk; the window shows the counts, not this list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ScanSummary {
    pub slicer: String,
    pub label: String,
    pub found: bool,
    pub printers: u32,
    pub filaments: u32,
    pub processes: u32,
    pub warnings: u32,
    pub warning_notes: Vec<String>,
    pub paths: Vec<String>,
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

/// Which slicers have a config directory. This only checks that the directory
/// exists. Counting and reading profiles waits until the user asks.
pub fn detect_installs() -> Vec<SlicerInstall> {
    let mut out = Vec::new();
    for (id, label, paths) in candidate_paths() {
        if let Some(root) = paths.into_iter().find(|path| path.is_dir()) {
            out.push(SlicerInstall {
                id,
                label,
                found: true,
                config_root: Some(root.display().to_string()),
                printers: 0,
                filaments: 0,
                processes: 0,
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

/// Walk one install and classify its profiles. Does not resolve inheritance.
pub fn scan_presets(slicer_id: &str, progress: &mut dyn FnMut(&ScanCounts)) -> Result<ScanSummary, String> {
    let Some((_, label, _)) = candidate_paths().into_iter().find(|(id, _, _)| id == slicer_id) else {
        return Err(format!("unknown slicer id `{slicer_id}`"));
    };
    let Some(root) = resolve_config_root(slicer_id) else {
        progress(&ScanCounts::default());
        return Ok(ScanSummary {
            slicer: slicer_id.to_string(),
            label,
            found: false,
            ..ScanSummary::default()
        });
    };
    scan_config_root(slicer_id, &label, &root, progress)
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

pub fn list_preset_offers(slicer_id: &str) -> Result<Vec<PresetOffer>, String> {
    let Some(config_root) = resolve_config_root(slicer_id) else {
        return Ok(Vec::new());
    };
    let mut offers = Vec::new();
    match slicer_id {
        "bambu" | "orca" => {
            // Names come from the file name. Resolving inheritance here used to
            // re-read the whole library for every file, which froze the window.
            for (kind, path) in discover_user_json(&config_root) {
                let stem = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("preset");
                offers.push(PresetOffer {
                    id: format!("{slicer_id}:{kind}:{}", path.display()),
                    path: path.display().to_string(),
                    kind: kind.clone(),
                    name: format!("{stem} ({kind})"),
                    slicer: slicer_id.to_string(),
                    supported: true,
                    note: None,
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
    let text = serde_json::to_string(&flat).map_err(|e| e.to_string())?;
    std::fs::write(target, text).map_err(|e| e.to_string())
}

pub fn import_presets(
    slicer_id: &str,
    selected_paths: &[String],
    imported_tree: &Path,
    shale_preset_dir: &Path,
) -> Result<SlicerImportReport, String> {
    import_presets_with_progress(slicer_id, selected_paths, imported_tree, shale_preset_dir, &mut |_| {})
}

pub fn import_presets_with_progress(
    slicer_id: &str,
    selected_paths: &[String],
    imported_tree: &Path,
    shale_preset_dir: &Path,
    progress: &mut dyn FnMut(&ScanCounts),
) -> Result<SlicerImportReport, String> {
    let Some(config_root) = resolve_config_root(slicer_id) else {
        return Err(format!("`{slicer_id}` is not installed on this machine"));
    };
    if slicer_id == "bambu" || slicer_id == "orca" {
        return import_bambu_family(&config_root, selected_paths, imported_tree, progress);
    }
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

fn scan_config_root(
    slicer_id: &str,
    label: &str,
    root: &Path,
    progress: &mut dyn FnMut(&ScanCounts),
) -> Result<ScanSummary, String> {
    let files = match slicer_id {
        "bambu" | "orca" => discover_user_json(root),
        "prusa" => discover_prusa_presets(root),
        "cura" => discover_cura_presets(root),
        _ => return Err(format!("unknown slicer id `{slicer_id}`")),
    };
    let total = files.len() as u32;
    let mut counts = ScanCounts {
        total,
        ..ScanCounts::default()
    };
    progress(&counts);
    let known = if slicer_id == "bambu" || slicer_id == "orca" {
        known_profile_stems(root)
    } else {
        std::collections::HashSet::new()
    };
    let mut notes = Vec::new();
    let mut paths = Vec::with_capacity(files.len());
    for (index, (kind, path)) in files.iter().enumerate() {
        match kind.as_str() {
            "machine" => counts.printers += 1,
            "filament" => counts.filaments += 1,
            _ => counts.processes += 1,
        }
        paths.push(path.display().to_string());
        if slicer_id == "bambu" || slicer_id == "orca" {
            if let Some(parent) = peek_inherits(path) {
                if !known.contains(&parent.to_ascii_lowercase()) {
                    counts.warnings += 1;
                    if notes.len() < 12 {
                        let stem = path
                            .file_stem()
                            .and_then(|name| name.to_str())
                            .unwrap_or("preset");
                        notes.push(format!("{stem}: parent `{parent}` is not on this machine"));
                    }
                }
            }
        }
        counts.seen = index as u32 + 1;
        if counts.seen == total || counts.seen.is_multiple_of(40) {
            progress(&counts);
        }
    }
    Ok(ScanSummary {
        slicer: slicer_id.to_string(),
        label: label.to_string(),
        found: true,
        printers: counts.printers,
        filaments: counts.filaments,
        processes: counts.processes,
        warnings: counts.warnings,
        warning_notes: notes,
        paths,
    })
}

fn known_profile_stems(root: &Path) -> std::collections::HashSet<String> {
    let bundled = bundled_vendor_dirs();
    let user = vec![root.to_path_buf()];
    let mut stems = std::collections::HashSet::new();
    for kind in ["machine", "filament", "process"] {
        for dir in orca_family_lookup_dirs(&bundled, &user, kind) {
            let Ok(read) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in read.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|name| name.to_str()) {
                    stems.insert(stem.to_ascii_lowercase());
                }
            }
        }
    }
    stems
}

/// The parent name, if the first 4 KiB of the file names one. A full parse is
/// not needed to decide whether the parent exists.
fn peek_inherits(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 4096];
    let read = file.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..read]);
    let key_at = text.find("\"inherits\"")?;
    let rest = &text[key_at + "\"inherits\"".len()..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    if after.starts_with("null") || after.starts_with('}') {
        return None;
    }
    let open = after.find('"')?;
    let body = &after[open + 1..];
    let close = body.find('"')?;
    let name = &body[..close];
    if name.trim().is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn import_bambu_family(
    config_root: &Path,
    selected_paths: &[String],
    imported_tree: &Path,
    progress: &mut dyn FnMut(&ScanCounts),
) -> Result<SlicerImportReport, String> {
    use std::collections::HashMap;
    let bundled = bundled_vendor_dirs();
    let user_dirs = vec![config_root.to_path_buf()];
    let mut by_kind: HashMap<String, Vec<(String, PathBuf)>> = HashMap::new();
    let mut report = SlicerImportReport::default();
    for path_str in selected_paths {
        let path = PathBuf::from(path_str);
        if !path.is_file() {
            report.skipped += 1;
            push_note(
                &mut report,
                path_str,
                path_str,
                "skipped",
                Some("file not found".into()),
            );
            continue;
        }
        let kind = infer_json_kind(&path).unwrap_or_else(|| "process".to_string());
        by_kind.entry(kind).or_default().push((path_str.clone(), path));
    }
    let total: u32 = by_kind.values().map(|files| files.len() as u32).sum();
    let mut seen = 0u32;
    let mut imported_printers = 0u32;
    let mut imported_filaments = 0u32;
    let mut imported_processes = 0u32;
    progress(&ScanCounts {
        seen,
        total,
        ..ScanCounts::default()
    });
    for (kind, files) in &by_kind {
        let lookup = orca_family_lookup_dirs(&bundled, &user_dirs, kind);
        for chunk in files.chunks(40) {
            let paths: Vec<PathBuf> = chunk.iter().map(|(_, path)| path.clone()).collect();
            let resolved = flatten_presets_shared(&paths, &lookup);
            for ((path_str, path), merged) in chunk.iter().zip(resolved) {
                seen += 1;
                match merged {
                    Ok(merged) => {
                        let stem = safe_file_stem(path);
                        let dest = imported_tree
                            .join("BBL")
                            .join(kind)
                            .join(format!("imported_{stem}.json"));
                        if let Err(err) = write_merged_json(&dest, &merged) {
                            report.skipped += 1;
                            push_note(&mut report, path_str, path_str, "skipped", Some(err));
                        } else {
                            report.imported += 1;
                            match kind.as_str() {
                                "machine" => imported_printers += 1,
                                "filament" => imported_filaments += 1,
                                _ => imported_processes += 1,
                            }
                        }
                    }
                    Err(err) => {
                        report.skipped += 1;
                        push_note(&mut report, path_str, path_str, "skipped", Some(err));
                    }
                }
                if seen == total || seen.is_multiple_of(40) {
                    progress(&ScanCounts {
                        seen,
                        total,
                        printers: imported_printers,
                        filaments: imported_filaments,
                        processes: imported_processes,
                        warnings: report.skipped,
                    });
                }
            }
        }
    }
    Ok(report)
}

fn push_note(
    report: &mut SlicerImportReport,
    path: &str,
    name: &str,
    outcome: &str,
    detail: Option<String>,
) {
    if report.lines.len() >= 40 {
        return;
    }
    report.lines.push(ImportLine {
        path: path.to_string(),
        name: name.to_string(),
        outcome: outcome.to_string(),
        detail,
    });
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
    use crate::bambu::flatten_preset_from_dirs;
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
            .and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
            })
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
        std::env::set_var("APPDATA", &appdata);
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
        set_test_profiles_root(Some(bundled.clone()));
        let broken = fixture_root().join("broken-user/BambuStudio");
        let preset = broken.join("user/default/process/orphan.json");
        let lookup = orca_family_lookup_dirs(
            &[bundled.join("BBL")],
            std::slice::from_ref(&broken),
            "process",
        );
        assert!(flatten_preset_from_dirs(&preset, &lookup).is_err());
        set_test_profiles_root(None);
    }

    fn write_profile_tree(root: &std::path::Path, count: usize) {
        let dir = root.join("user").join("default").join("process");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("base.json"),
            r#"{"name":"Base","layer_height":"0.2"}"#,
        )
        .unwrap();
        for index in 0..count {
            std::fs::write(
                dir.join(format!("child_{index}.json")),
                format!(
                    r#"{{"name":"Child {index}","inherits":"base","layer_height":"0.16"}}"#
                ),
            )
            .unwrap();
        }
    }

    #[test]
    fn scan_of_two_thousand_profiles_streams_progress() {
        let root = std::env::temp_dir().join(format!("shale-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_profile_tree(&root, 2000);
        let started = std::time::Instant::now();
        let mut ticks = 0u32;
        let summary = scan_config_root("bambu", "Bambu Studio", &root, &mut |_| ticks += 1).unwrap();
        let elapsed = started.elapsed();
        let _ = std::fs::remove_dir_all(&root);
        assert!(summary.processes >= 2000, "counted {}", summary.processes);
        assert!(ticks >= 2, "progress ticks {ticks}");
        assert!(
            elapsed.as_secs() < 3,
            "scan of 2000 profiles took {elapsed:?}"
        );
        eprintln!("scan after (2000 profiles): {elapsed:?}, ticks {ticks}");
    }

    #[test]
    fn shared_index_import_beats_per_file_flatten() {
        let root = std::env::temp_dir().join(format!("shale-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let count = 120usize;
        write_profile_tree(&root, count);
        let dir = root.join("user").join("default").join("process");
        let paths: Vec<std::path::PathBuf> = (0..count)
            .map(|index| dir.join(format!("child_{index}.json")))
            .collect();
        let lookup = vec![dir.clone()];
        let started = std::time::Instant::now();
        for path in &paths {
            flatten_preset_from_dirs(path, &lookup).unwrap();
        }
        let before = started.elapsed();
        let started = std::time::Instant::now();
        let resolved = flatten_presets_shared(&paths, &lookup);
        let after = started.elapsed();
        assert_eq!(resolved.len(), count);
        assert!(resolved.iter().all(|item| item.is_ok()));
        let dest = root.join("out");
        let selected: Vec<String> = paths.iter().map(|path| path.display().to_string()).collect();
        let started = std::time::Instant::now();
        let report = import_bambu_family(&root, &selected, &dest, &mut |_| {}).unwrap();
        let written = started.elapsed();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(report.imported, count as u32);
        assert!(
            after.as_millis() * 4 < before.as_millis().max(1),
            "shared index {after:?} was not faster than per-file flatten {before:?}"
        );
        eprintln!("import before (per-file flatten, n={count}): {before:?}");
        eprintln!("import after (shared index, n={count}): {after:?}");
        eprintln!("import after including writes (n={count}): {written:?}");
    }

    /// Opt-in timing against the real bundled profile library. Not part of CI:
    /// `cargo test -p shale-core --lib profile_against_bundled_library -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn profile_against_bundled_library() {
        let profiles = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/BambuStudio/resources/profiles");
        if !profiles.join("BBL/process").is_dir() {
            eprintln!("bundled profiles not present, skipping");
            return;
        }
        set_test_profiles_root(Some(profiles));
        let root = std::env::temp_dir().join(format!("shale-real-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let count = 40usize;
        write_profile_tree(&root, count);
        let dir = root.join("user").join("default").join("process");
        let paths: Vec<PathBuf> = (0..count)
            .map(|index| dir.join(format!("child_{index}.json")))
            .collect();
        let lookup = orca_family_lookup_dirs(&bundled_vendor_dirs(), std::slice::from_ref(&root), "process");
        let started = std::time::Instant::now();
        for path in &paths {
            let _ = flatten_preset_from_dirs(path, &lookup);
        }
        let before = started.elapsed();
        let started = std::time::Instant::now();
        let _ = flatten_presets_shared(&paths, &lookup);
        let after = started.elapsed();
        eprintln!(
            "real library, n={count}, lookup dirs {}: before {before:?}, after {after:?}",
            lookup.len()
        );
        let wide = 2000usize;
        write_profile_tree(&root, wide);
        let wide_paths: Vec<PathBuf> = (0..wide)
            .map(|index| dir.join(format!("child_{index}.json")))
            .collect();
        let lookup = orca_family_lookup_dirs(&bundled_vendor_dirs(), std::slice::from_ref(&root), "process");
        let started = std::time::Instant::now();
        let resolved = flatten_presets_shared(&wide_paths, &lookup);
        let wide_after = started.elapsed();
        let ok = resolved.iter().filter(|item| item.is_ok()).count();
        eprintln!("real library shared index, n={wide}: {wide_after:?}, resolved {ok}");
        let _ = std::fs::remove_dir_all(&root);
        set_test_profiles_root(None);
    }
}