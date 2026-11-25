//! Settings embedded in a Bambu or Orca 3MF (`Metadata/project_settings.config`).
//!
//! A project file carries hundreds of keys. Shale copies the ones it can hand
//! to CuraEngine and leaves the rest on the printer profile. A key the user
//! has already set is not overwritten: user overrides win, then the project,
//! then the printer and filament defaults.

use std::io::Cursor;

use serde::Serialize;
use serde_json::Value as Json;
use zip::ZipArchive;

use crate::mesh::read_zip_text;
use crate::resolver::UserSettings;

/// One setting taken from the project file, in the form CuraEngine expects.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImportedSetting {
    /// Cura setting key.
    pub key: String,
    pub label: String,
    /// Value passed to the engine.
    pub value: String,
    /// The same value, written for a person.
    pub detail: String,
}

/// What a project file contributed, and how much of it was left alone.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProjectSettingsImport {
    pub applied: Vec<ImportedSetting>,
    /// Keys in the file that were not mapped.
    pub ignored: usize,
    pub note: String,
}

/// The mapped values, ready to merge into a [`UserSettings`].
#[derive(Clone, Debug)]
pub struct ProjectSettings {
    pub values: Vec<(String, String)>,
    pub summary: ProjectSettingsImport,
}

/// Read `Metadata/project_settings.config` from a 3MF. `None` when the file
/// has no such config, or it is not a JSON object.
pub fn read_project_settings(bytes: &[u8]) -> Option<ProjectSettings> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).ok()?;
    let text = read_zip_text(&mut archive, "Metadata/project_settings.config").ok()?;
    let json: Json = serde_json::from_str(&text).ok()?;
    let object = json.as_object()?;
    let mut values = Vec::new();
    let mut applied = Vec::new();

    push_number(
        object.get("layer_height"),
        "layer_height",
        "Layer height",
        "mm",
        &mut values,
        &mut applied,
    );
    push_number(
        object.get("wall_loops"),
        "wall_line_count",
        "Wall loops",
        "",
        &mut values,
        &mut applied,
    );
    if let Some(raw) = object.get("sparse_infill_density") {
        if let Some(density) = number(raw) {
            let value = format_number(density);
            values.push(("infill_sparse_density".to_string(), value.clone()));
            applied.push(ImportedSetting {
                key: "infill_sparse_density".to_string(),
                label: "Infill density".to_string(),
                value,
                detail: format!("{density:.0}%"),
            });
        }
    }
    if let Some(pattern) = object.get("sparse_infill_pattern").and_then(Json::as_str) {
        let mapped = cura_infill(pattern);
        values.push(("infill_pattern".to_string(), mapped.clone()));
        applied.push(ImportedSetting {
            key: "infill_pattern".to_string(),
            label: "Infill pattern".to_string(),
            value: mapped,
            detail: pattern.to_string(),
        });
    }
    if let Some(enabled) = flag(object.get("enable_support")) {
        let value = if enabled { "1" } else { "0" };
        values.push(("support_enable".to_string(), value.to_string()));
        applied.push(ImportedSetting {
            key: "support_enable".to_string(),
            label: "Supports".to_string(),
            value: value.to_string(),
            detail: if enabled { "on" } else { "off" }.to_string(),
        });
    }
    push_number(
        object.get("nozzle_temperature"),
        "material_print_temperature",
        "Nozzle temperature",
        "°C",
        &mut values,
        &mut applied,
    );
    let bed = object
        .get("textured_plate_temp")
        .or_else(|| object.get("hot_plate_temp"));
    push_number(
        bed,
        "material_bed_temperature",
        "Bed temperature",
        "°C",
        &mut values,
        &mut applied,
    );

    let mapped_keys = [
        "layer_height",
        "wall_loops",
        "sparse_infill_density",
        "sparse_infill_pattern",
        "enable_support",
        "nozzle_temperature",
        "textured_plate_temp",
        "hot_plate_temp",
    ];
    let mapped_present = mapped_keys
        .iter()
        .filter(|key| object.contains_key(**key))
        .count();
    let ignored = object.len().saturating_sub(mapped_present);
    let note = format!(
        "{ignored} machine, filament and process keys were left on the printer profile"
    );
    Some(ProjectSettings {
        values,
        summary: ProjectSettingsImport {
            applied,
            ignored,
            note,
        },
    })
}

/// Copy project values into `user` where the user has not already set them.
pub fn apply_project_settings(user: &mut UserSettings, project: &ProjectSettings) {
    for (key, value) in &project.values {
        if user.extra.iter().any(|(existing, _)| existing == key) {
            continue;
        }
        match key.as_str() {
            "layer_height" if user.layer_height.is_none() => {
                if let Ok(parsed) = value.parse() {
                    user.layer_height = Some(parsed);
                }
            }
            "layer_height" => {}
            "infill_sparse_density" if user.infill_sparse_density.is_none() => {
                if let Ok(parsed) = value.parse() {
                    user.infill_sparse_density = Some(parsed);
                }
            }
            "infill_sparse_density" => {}
            "support_enable" if user.support_enable.is_none() => {
                user.support_enable = Some(value == "1" || value.eq_ignore_ascii_case("true"));
            }
            "support_enable" => {}
            "material_print_temperature" if user.material_print_temperature.is_none() => {
                if let Ok(parsed) = value.parse() {
                    user.material_print_temperature = Some(parsed);
                }
            }
            "material_print_temperature" => {}
            "material_bed_temperature" if user.material_bed_temperature.is_none() => {
                if let Ok(parsed) = value.parse() {
                    user.material_bed_temperature = Some(parsed);
                }
            }
            "material_bed_temperature" => {}
            _ => user.extra.push((key.clone(), value.clone())),
        }
    }
}

fn push_number(
    raw: Option<&Json>,
    key: &str,
    label: &str,
    unit: &str,
    values: &mut Vec<(String, String)>,
    applied: &mut Vec<ImportedSetting>,
) {
    let Some(parsed) = raw.and_then(number) else {
        return;
    };
    let value = format_number(parsed);
    let detail = if unit.is_empty() {
        value.clone()
    } else {
        format!("{value} {unit}")
    };
    values.push((key.to_string(), value.clone()));
    applied.push(ImportedSetting {
        key: key.to_string(),
        label: label.to_string(),
        value,
        detail,
    });
}

fn number(value: &Json) -> Option<f64> {
    match value {
        Json::Number(parsed) => parsed.as_f64(),
        Json::String(text) => text.trim().trim_end_matches('%').trim().parse().ok(),
        Json::Array(items) => items.iter().find_map(number),
        _ => None,
    }
}

fn flag(value: Option<&Json>) -> Option<bool> {
    match value? {
        Json::Bool(parsed) => Some(*parsed),
        Json::Number(parsed) => Some(parsed.as_f64()? != 0.0),
        Json::String(text) => match text.trim() {
            "1" | "true" | "True" | "yes" => Some(true),
            "0" | "false" | "False" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn format_number(value: f64) -> String {
    if (value - value.round()).abs() < 1e-6 {
        format!("{}", value.round() as i64)
    } else {
        let text = format!("{value:.4}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
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
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_overrides_win_and_the_rest_is_counted() {
        let json = r#"{
            "layer_height": "0.08",
            "wall_loops": 4,
            "sparse_infill_density": "15%",
            "sparse_infill_pattern": "grid",
            "enable_support": "0",
            "nozzle_temperature": ["220", "210"],
            "textured_plate_temp": ["55"],
            "printer_model": "Bambu Lab A1",
            "filament_type": ["PLA"]
        }"#;
        let bytes = zip_with_settings(json);
        let project = read_project_settings(&bytes).unwrap();
        assert_eq!(project.summary.applied.len(), 7);
        assert_eq!(project.summary.ignored, 2);
        assert!(project.summary.note.contains("2 machine"));

        let mut user = UserSettings {
            layer_height: Some(0.2),
            ..UserSettings::default()
        };
        user.extra
            .push(("wall_line_count".to_string(), "2".to_string()));
        apply_project_settings(&mut user, &project);
        assert_eq!(user.layer_height, Some(0.2));
        assert!(user
            .extra
            .iter()
            .any(|(key, value)| key == "wall_line_count" && value == "2"));
        assert_eq!(user.infill_sparse_density, Some(15.0));
        assert_eq!(user.support_enable, Some(false));
        assert_eq!(user.material_print_temperature, Some(220.0));
        assert_eq!(user.material_bed_temperature, Some(55.0));
        assert!(user
            .extra
            .iter()
            .any(|(key, value)| key == "infill_pattern" && value == "grid"));
    }

    fn zip_with_settings(json: &str) -> Vec<u8> {
        use std::io::Write;
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            writer
                .start_file(
                    "Metadata/project_settings.config",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(json.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }
}
