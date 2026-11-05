//! A `.shale` project is a zip archive.
//!
//! `project.json` holds the printer, setting overrides, transforms, custom
//! instructions, and support choices. Model bytes are stored under `models/`
//! with their original extension, so a STEP file stays STEP. `thumbnail.jpg`
//! is a small preview. Nothing here talks to CuraEngine.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::Error;
use crate::instructions::CustomInstruction;
use crate::mesh::Transform;
use crate::resolver::UserSettings;
use crate::supports::SupportPlan;

const FORMAT: u32 = 1;
const RECENT_LIMIT: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectModel {
    pub name: String,
    pub bytes: Vec<u8>,
    pub transform: Transform,
    /// Per-part overrides, kept so a saved plate slices the same way twice.
    #[serde(default)]
    pub settings: std::collections::BTreeMap<String, String>,
    /// Which AMS spool printed this part.
    #[serde(default)]
    pub slot: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectData {
    pub name: String,
    pub printer_id: String,
    pub printer_label: String,
    pub variant_name: Option<String>,
    pub quality_type: Option<String>,
    pub material_id: Option<String>,
    /// The spools in the AMS beyond the plate's own material, in slot order.
    #[serde(default)]
    pub ams: Vec<String>,
    pub settings: UserSettings,
    pub chord_mm: f64,
    pub turn_deg: f64,
    pub models: Vec<ProjectModel>,
    pub instructions: Vec<CustomInstruction>,
    pub support: Option<SupportPlan>,
    /// Index into [`ProjectData::models`] the support plan belongs to.
    pub support_model: Option<usize>,
    pub thumbnail: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct LoadedProject {
    pub data: ProjectData,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Document {
    format: u32,
    name: String,
    printer_id: String,
    #[serde(default)]
    printer_label: String,
    #[serde(default)]
    variant_name: Option<String>,
    #[serde(default)]
    quality_type: Option<String>,
    #[serde(default)]
    material_id: Option<String>,
    #[serde(default)]
    ams: Vec<String>,
    #[serde(default)]
    settings: UserSettings,
    #[serde(default)]
    chord_mm: f64,
    #[serde(default)]
    turn_deg: f64,
    models: Vec<DocumentModel>,
    #[serde(default)]
    instructions: Vec<CustomInstruction>,
    #[serde(default)]
    support: Option<SupportPlan>,
    #[serde(default)]
    support_model: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DocumentModel {
    file: String,
    name: String,
    transform: Transform,
    #[serde(default)]
    settings: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    slot: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RecentFile {
    entries: Vec<StoredRecent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRecent {
    path: String,
    name: String,
    saved_at_ms: u64,
    printer: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: String,
    pub name: String,
    pub saved_at_ms: u64,
    pub printer: String,
    pub missing: bool,
}

pub fn write_project(path: &Path, data: &ProjectData) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut stored = Vec::new();
    let mut document_models = Vec::new();
    for (index, model) in data.models.iter().enumerate() {
        let file = archive_name(index, &model.name);
        if model.bytes.is_empty() {
            return Err(Error::message(format!(
                "Cannot save {name}: the original file bytes are missing.",
                name = model.name
            )));
        }
        stored.push((file.clone(), model.bytes.clone()));
        document_models.push(DocumentModel {
            file,
            name: model.name.clone(),
            transform: model.transform.clone(),
            settings: model.settings.clone(),
            slot: model.slot,
        });
    }
    if let Some(index) = data.support_model {
        if index >= data.models.len() {
            return Err(Error::message(
                "The support plan does not point at a model in this project.",
            ));
        }
    }
    let mut support = data.support.clone();
    if let Some(plan) = support.as_mut() {
        for span in &mut plan.spans {
            span.samples.clear();
        }
    }
    let document = Document {
        format: FORMAT,
        name: data.name.clone(),
        printer_id: data.printer_id.clone(),
        printer_label: data.printer_label.clone(),
        variant_name: data.variant_name.clone(),
        quality_type: data.quality_type.clone(),
        material_id: data.material_id.clone(),
        ams: data.ams.clone(),
        settings: data.settings.clone(),
        chord_mm: data.chord_mm,
        turn_deg: data.turn_deg,
        models: document_models,
        instructions: data.instructions.clone(),
        support,
        support_model: data.support_model,
    };
    let json = serde_json::to_vec_pretty(&document).map_err(|source| Error::Json {
        path: path.display().to_string(),
        source,
    })?;
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("project.json", options)
        .map_err(|err| Error::message(format!("Could not write project.json: {err}")))?;
    zip.write_all(&json)?;
    for (name, bytes) in &stored {
        zip.start_file(name.as_str(), options)
            .map_err(|err| Error::message(format!("Could not write {name}: {err}")))?;
        zip.write_all(bytes)?;
    }
    zip.start_file("thumbnail.jpg", options)
        .map_err(|err| Error::message(format!("Could not write thumbnail.jpg: {err}")))?;
    zip.write_all(&data.thumbnail)?;
    zip.finish()
        .map_err(|err| Error::message(format!("Could not finish the project archive: {err}")))
        .map(|_| ())
}

pub fn read_project(path: &Path) -> Result<LoadedProject, Error> {
    if !path.is_file() {
        return Err(Error::message(format!(
            "That project file is missing: {}",
            path.display()
        )));
    }
    let file = File::open(path)?;
    let mut zip = ZipArchive::new(file)
        .map_err(|err| Error::message(format!("Could not read the project archive: {err}")))?;
    let json = read_zip(&mut zip, "project.json").map_err(|_| {
        Error::message("The project archive has no project.json.")
    })?;
    let document: Document = serde_json::from_slice(&json).map_err(|source| Error::Json {
        path: path.display().to_string(),
        source,
    })?;
    if document.format != FORMAT {
        return Err(Error::message(format!(
            "This project uses format {}, and this version of Shale reads format {FORMAT}.",
            document.format
        )));
    }
    let mut warnings = Vec::new();
    let mut models = Vec::new();
    for entry in &document.models {
        if entry.file.contains("..") || entry.file.starts_with('/') {
            return Err(Error::message(format!(
                "The project lists an unsafe model path: {}",
                entry.file
            )));
        }
        let bytes = match read_zip(&mut zip, &entry.file) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => {
                return Err(Error::message(format!(
                    "The project is missing {}: the archive entry is empty.",
                    entry.file
                )));
            }
            Err(_) => {
                return Err(Error::message(format!(
                    "The project is missing {}. The archive does not contain that model.",
                    entry.file
                )));
            }
        };
        models.push(ProjectModel {
            name: entry.name.clone(),
            bytes,
            transform: entry.transform.clone(),
            settings: entry.settings.clone(),
            slot: entry.slot,
        });
    }
    let thumbnail = match read_zip(&mut zip, "thumbnail.jpg") {
        Ok(bytes) => bytes,
        Err(_) => {
            warnings.push("This project has no thumbnail.".into());
            Vec::new()
        }
    };
    Ok(LoadedProject {
        data: ProjectData {
            name: document.name,
            printer_id: document.printer_id,
            printer_label: document.printer_label,
            variant_name: document.variant_name,
            quality_type: document.quality_type,
            material_id: document.material_id,
            ams: document.ams,
            settings: document.settings,
            chord_mm: document.chord_mm,
            turn_deg: document.turn_deg,
            models,
            instructions: document.instructions,
            support: document.support,
            support_model: document.support_model,
            thumbnail,
        },
        warnings,
    })
}

pub fn load_recents(dir: &Path) -> Result<Vec<RecentEntry>, Error> {
    let path = recent_path(dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    let file: RecentFile = serde_json::from_str(&text).map_err(|source| Error::Json {
        path: path.display().to_string(),
        source,
    })?;
    Ok(file
        .entries
        .into_iter()
        .take(RECENT_LIMIT)
        .map(|entry| RecentEntry {
            missing: !Path::new(&entry.path).is_file(),
            path: entry.path,
            name: entry.name,
            saved_at_ms: entry.saved_at_ms,
            printer: entry.printer,
        })
        .collect())
}

pub fn remember_recent(dir: &Path, entry: &RecentEntry) -> Result<Vec<RecentEntry>, Error> {
    fs::create_dir_all(dir)?;
    let mut entries = load_recents(dir)?;
    entries.retain(|existing| existing.path != entry.path);
    entries.insert(
        0,
        RecentEntry {
            path: entry.path.clone(),
            name: entry.name.clone(),
            saved_at_ms: entry.saved_at_ms,
            printer: entry.printer.clone(),
            missing: !Path::new(&entry.path).is_file(),
        },
    );
    entries.truncate(RECENT_LIMIT);
    write_recents(dir, &entries)?;
    Ok(entries)
}

pub fn forget_recent(dir: &Path, path: &str) -> Result<Vec<RecentEntry>, Error> {
    fs::create_dir_all(dir)?;
    let mut entries = load_recents(dir).unwrap_or_default();
    entries.retain(|entry| entry.path != path);
    write_recents(dir, &entries)?;
    Ok(entries)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn write_recents(dir: &Path, entries: &[RecentEntry]) -> Result<(), Error> {
    let stored = RecentFile {
        entries: entries
            .iter()
            .map(|entry| StoredRecent {
                path: entry.path.clone(),
                name: entry.name.clone(),
                saved_at_ms: entry.saved_at_ms,
                printer: entry.printer.clone(),
            })
            .collect(),
    };
    let path = recent_path(dir);
    let json = serde_json::to_vec_pretty(&stored).map_err(|source| Error::Json {
        path: path.display().to_string(),
        source,
    })?;
    fs::write(path, json)?;
    Ok(())
}

fn recent_path(dir: &Path) -> PathBuf {
    dir.join("recents.json")
}

fn archive_name(index: usize, name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model");
    let mut clean = String::new();
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            clean.push(ch);
        } else {
            clean.push('_');
        }
    }
    if clean.is_empty() || clean.starts_with('.') {
        clean = format!("model-{index}");
    }
    format!("models/{index}-{clean}")
}

fn read_zip(zip: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>, ()> {
    let mut file = zip.by_name(name).map_err(|_| ())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|_| ())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instructions::CustomInstruction;
    use crate::mesh::{write_binary_stl, TriangleMesh};
    use crate::supports::{
        FeatureKind, RegionRole, SupportComparison, SupportParams, SupportPlan, SupportRegion,
    };

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shale-project-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn triangle() -> Vec<u8> {
        let mesh = TriangleMesh {
            vertices: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 8.0, 4.0]],
            triangles: vec![[0, 1, 2]],
        };
        let path = std::env::temp_dir().join(format!("shale-project-tri-{}.stl", std::process::id()));
        write_binary_stl(&path, &mesh).unwrap();
        fs::read(&path).unwrap()
    }

    fn sample_support() -> SupportPlan {
        SupportPlan {
            params: SupportParams::default(),
            regions: vec![SupportRegion {
                id: 1,
                kind: FeatureKind::Cantilever,
                role: RegionRole::Enforce,
                enabled: false,
                min: [1.0, 2.0, 0.0],
                max: [4.0, 5.0, 6.0],
                contact_area_mm2: 3.5,
                volume_mm3: 9.0,
                summary: "tip".into(),
            }],
            comparison: SupportComparison {
                proposed_volume_mm3: 9.0,
                proposed_contact_mm2: 3.5,
                proposed_time_seconds: 12.0,
                everywhere_volume_mm3: 40.0,
                everywhere_contact_mm2: 10.0,
                everywhere_time_seconds: 30.0,
                times_from_engine: false,
                proposed_support_metres: None,
                everywhere_support_metres: None,
            },
            spans: Vec::new(),
        }
    }

    #[test]
    fn project_round_trip_keeps_step_bytes_and_choices() {
        let dir = temp_dir("round");
        let path = dir.join("bracket.shale");
        let stl = triangle();
        let step = include_bytes!("../../../examples/block-20.step").to_vec();
        let signature = b"ISO-10303-21";
        assert!(
            step.windows(signature.len())
                .any(|window| window == signature),
            "the sample STEP file should be a STEP exchange"
        );
        let saved = ProjectData {
            name: "bracket".into(),
            printer_id: "creality_ender3".into(),
            printer_label: "Creality Ender-3".into(),
            variant_name: Some("0.4mm Nozzle".into()),
            quality_type: Some("draft".into()),
            material_id: Some("generic_pla".into()),
            ams: vec!["generic_petg".into(), String::new(), "generic_abs".into()],
            settings: UserSettings {
                layer_height: Some(0.16),
                infill_sparse_density: Some(18.0),
                support_enable: Some(true),
                material_print_temperature: Some(210.0),
                material_bed_temperature: Some(60.0),
                extra: vec![("adhesion_type".into(), "brim".into())],
            },
            chord_mm: 0.08,
            turn_deg: 12.0,
            models: vec![
                ProjectModel {
                    name: "cube.stl".into(),
                    bytes: stl.clone(),
                    transform: Transform {
                        translation: [10.0, 4.0, 0.0],
                        rotation_deg: [0.0, 90.0, 0.0],
                        scale: [1.0, 1.0, 2.0],
                    },
                    settings: Default::default(),
                    slot: Some(1),
                },
                ProjectModel {
                    name: "block-20.step".into(),
                    bytes: step.clone(),
                    transform: Transform::default(),
                    settings: Default::default(),
                    slot: None,
                },
            ],
            instructions: vec![CustomInstruction::Beep { layer: 4 }],
            support: Some(sample_support()),
            support_model: Some(0),
            thumbnail: b"thumbnail-bytes".to_vec(),
        };
        write_project(&path, &saved).unwrap();
        let loaded = read_project(&path).unwrap();
        assert!(loaded.warnings.is_empty());
        let data = loaded.data;
        assert_eq!(data.name, saved.name);
        assert_eq!(data.printer_id, saved.printer_id);
        assert_eq!(data.printer_label, saved.printer_label);
        assert_eq!(data.variant_name, saved.variant_name);
        assert_eq!(data.quality_type, saved.quality_type);
        assert_eq!(data.material_id, saved.material_id);
        // The AMS is part of the plate: a spool list and which spool each part
        // prints from have to survive a save and a reopen.
        assert_eq!(data.ams, saved.ams);
        assert_eq!(data.models[0].slot, Some(1));
        assert_eq!(data.models[1].slot, None);
        assert_eq!(data.settings, saved.settings);
        assert_eq!(data.chord_mm, saved.chord_mm);
        assert_eq!(data.turn_deg, saved.turn_deg);
        assert_eq!(data.models.len(), 2);
        assert_eq!(data.models[0].name, "cube.stl");
        assert_eq!(data.models[0].bytes, stl);
        assert_eq!(data.models[0].transform, saved.models[0].transform);
        assert_eq!(data.models[1].name, "block-20.step");
        assert_eq!(data.models[1].bytes, step);
        assert!(data.models[1].name.ends_with(".step"));
        assert_eq!(data.instructions, saved.instructions);
        let support = data.support.expect("support choices");
        assert!(!support.regions[0].enabled);
        assert_eq!(support.regions[0].summary, "tip");
        assert_eq!(data.support_model, Some(0));
        assert_eq!(data.thumbnail, b"thumbnail-bytes");
    }

    #[test]
    fn missing_model_entry_is_an_error() {
        let dir = temp_dir("missing-entry");
        let path = dir.join("gap.shale");
        let mut raw = Vec::new();
        {
            let mut zip = ZipWriter::new(std::io::Cursor::new(&mut raw));
            let options = SimpleFileOptions::default();
            let json = br#"{"format":1,"name":"gap","printer_id":"creality_ender3","models":[{"file":"models/0-block.step","name":"block.step","transform":{"translation":[0,0,0],"rotation_deg":[0,0,0],"scale":[1,1,1]}}]}"#;
            zip.start_file("project.json", options).unwrap();
            zip.write_all(json).unwrap();
            zip.finish().unwrap();
        }
        fs::write(&path, raw).unwrap();
        let err = read_project(&path).unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("models/0-block.step"),
            "the missing model should be named: {text}"
        );
    }

    #[test]
    fn missing_project_file_is_an_error() {
        let path = std::env::temp_dir().join(format!(
            "shale-no-such-project-{}-{}.shale",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_file(&path);
        let err = read_project(&path).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("missing"), "{text}");
        assert!(text.contains(&path.display().to_string()), "{text}");
    }

    #[test]
    fn recents_keep_eight_and_mark_missing_files() {
        let dir = temp_dir("recents");
        let first = dir.join("one.shale");
        fs::write(&first, b"a").unwrap();
        let second = dir.join("two.shale");
        fs::write(&second, b"b").unwrap();
        remember_recent(
            &dir,
            &RecentEntry {
                path: first.display().to_string(),
                name: "one".into(),
                saved_at_ms: 10,
                printer: "Creality Ender-3".into(),
                missing: false,
            },
        )
        .unwrap();
        let listed = remember_recent(
            &dir,
            &RecentEntry {
                path: second.display().to_string(),
                name: "two".into(),
                saved_at_ms: 20,
                printer: "Prusa i3".into(),
                missing: false,
            },
        )
        .unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name, "two");
        assert!(!listed[0].missing);
        assert_eq!(listed[1].printer, "Creality Ender-3");
        fs::remove_file(&first).unwrap();
        let listed = load_recents(&dir).unwrap();
        assert!(listed.iter().any(|entry| entry.name == "one" && entry.missing));
        let listed = forget_recent(&dir, &first.display().to_string()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "two");

        for index in 0..10 {
            let path = dir.join(format!("extra-{index}.shale"));
            fs::write(&path, b"x").unwrap();
            remember_recent(
                &dir,
                &RecentEntry {
                    path: path.display().to_string(),
                    name: format!("extra-{index}"),
                    saved_at_ms: 100 + index as u64,
                    printer: "Prusa i3".into(),
                    missing: false,
                },
            )
            .unwrap();
        }
        let listed = load_recents(&dir).unwrap();
        assert_eq!(listed.len(), RECENT_LIMIT);
        assert_eq!(listed[0].name, "extra-9");
        assert!(listed.iter().all(|entry| entry.name != "two"));
    }
}
