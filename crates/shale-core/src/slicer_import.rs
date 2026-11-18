//! Detect other slicers on disk and summarise user preset folders for import.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
pub struct SlicerImportReport {
    pub source: String,
    pub imported_presets: u32,
    pub skipped: u32,
    pub messages: Vec<String>,
}

/// Which subfolder names count toward each bucket (case-insensitive).
const PRINTER_DIRS: &[&str] = &["machine", "printer", "printers", "snapshots"];
const FILAMENT_DIRS: &[&str] = &["filament", "filaments", "material"];
const PROCESS_DIRS: &[&str] = &["process", "processes", "print", "quality", "definition"];

fn push_candidate(
    id: &str,
    label: &str,
    root: PathBuf,
    out: &mut Vec<SlicerInstall>,
) {
    let (printers, filaments, processes) = if root.is_dir() {
        summarise_tree(&root)
    } else {
        (0, 0, 0)
    };
    let found = root.is_dir();
    out.push(SlicerInstall {
        id: id.to_string(),
        label: label.to_string(),
        found,
        config_root: found.then(|| root.display().to_string()),
        printers,
        filaments,
        processes,
    });
}

fn candidate_paths() -> Vec<(String, String, Vec<PathBuf>)> {
    let mut items: Vec<(String, String, Vec<PathBuf>)> = Vec::new();
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
    items.push(("bambu".into(), "Bambu Studio".into(), bambu));
    items.push(("orca".into(), "OrcaSlicer".into(), orca));
    items.push(("prusa".into(), "PrusaSlicer".into(), prusa));
    items.push(("cura".into(), "UltiMaker Cura".into(), cura));
    items
}

/// List known slicers and how many JSON presets sit under their user trees.
pub fn detect_installs() -> Vec<SlicerInstall> {
    let mut out = Vec::new();
    for (id, label, paths) in candidate_paths() {
        if let Some(root) = paths.into_iter().find(|path| path.is_dir()) {
            push_candidate(&id, &label, root, &mut out);
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

fn dir_matches(name: &str, allowed: &[&str]) -> bool {
    let lower = name.to_lowercase();
    allowed.iter().any(|entry| lower.contains(entry))
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
                if stack.len() < 12 {
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

/// Parse a small manifest listing preset files (used by tests and the wizard report).
pub fn parse_import_manifest(text: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|err| format!("manifest json: {err}"))?;
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

    fn write_json(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, body).expect("write");
    }

    #[test]
    fn manifest_lists_preset_names() {
        let text = r#"{"presets":["speed","draft",{"name":"fine"}]}"#;
        assert_eq!(
            parse_import_manifest(text).expect("parse"),
            vec!["speed".into(), "draft".into(), "fine".into()]
        );
    }

    #[test]
    fn summarise_counts_json_in_category_folders() {
        let dir = std::env::temp_dir().join(format!(
            "shale-slicer-import-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        write_json(&dir, "user/default/machine/a1.json", "{}");
        write_json(&dir, "user/default/filament/pla.json", "{}");
        write_json(&dir, "user/default/process/standard.json", "{}");
        write_json(&dir, "user/default/process/extra.json", "{}");
        let (p, f, pr) = summarise_tree(&dir);
        assert_eq!((p, f, pr), (1, 1, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_never_panics_without_appdata() {
        let _list = detect_installs();
    }
}