//! Named sets of slice settings: save what you changed, load it later.
//!
//! A preset is a plain map of Cura setting keys to values, with a name. It is
//! deliberately small: the engine takes settings by name, so a preset is
//! exactly the overrides the UI would have sent, and applying one is the same
//! operation as typing the values in by hand — which is what makes the
//! difference between a preset and the profile defaults something that can be
//! printed rather than described.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Why a preset was refused. Every variant names the thing that is wrong, so
/// the UI can show the sentence as written instead of guessing a cause.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetError {
    BlankName,
    /// The key is not one Shale sends to the engine. A preset carrying it would
    /// claim to do something and do nothing.
    UnknownKey(String),
    /// Two presets cannot share a file name.
    Exists(String),
    NotFound(String),
    Io(String),
    Json(String),
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PresetError::BlankName => write!(f, "a preset needs a name"),
            PresetError::UnknownKey(key) => write!(
                f,
                "`{key}` is not a slice setting Shale knows, so a preset holding it would change nothing"
            ),
            PresetError::Exists(name) => write!(f, "a preset called `{name}` already exists"),
            PresetError::NotFound(name) => write!(f, "there is no preset called `{name}`"),
            PresetError::Io(err) => write!(f, "the presets folder could not be read: {err}"),
            PresetError::Json(err) => write!(f, "a preset file is not readable JSON: {err}"),
        }
    }
}

impl std::error::Error for PresetError {}

/// One named set of overrides.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    pub values: BTreeMap<String, String>,
}

impl Preset {
    pub fn new(name: &str, values: BTreeMap<String, String>) -> Result<Self, PresetError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PresetError::BlankName);
        }
        Ok(Self {
            name: name.to_string(),
            values,
        })
    }

    /// The first key this preset carries that Shale does not know, if any.
    pub fn first_unknown<'a>(&'a self, known: &[&str]) -> Option<&'a str> {
        self.values
            .keys()
            .map(String::as_str)
            .find(|key| !known.contains(key))
    }

    /// Refuse the whole preset when any key is unknown: applying half a preset
    /// silently is worse than refusing it out loud.
    pub fn check(&self, known: &[&str]) -> Result<(), PresetError> {
        match self.first_unknown(known) {
            Some(key) => Err(PresetError::UnknownKey(key.to_string())),
            None => Ok(()),
        }
    }

    /// What this preset would change against the settings in hand.
    ///
    /// `current` is the value the UI is showing for each key — the resolved
    /// profile, or the override if there is one. A row is only here when the
    /// two differ, so an empty diff means "applying this changes nothing".
    pub fn diff<'a>(
        &'a self,
        current: &'a BTreeMap<String, String>,
    ) -> Vec<(String, Option<String>, String)> {
        self.values
            .iter()
            .filter(|(key, next)| current.get(*key) != Some(*next))
            .map(|(key, next)| {
                (
                    key.clone(),
                    current.get(key).cloned(),
                    next.clone(),
                )
            })
            .collect()
    }
}

/// A file name for a preset that cannot escape the folder.
///
/// Letters, digits, dashes and underscores survive; everything else, spaces
/// included, becomes a dash. The name inside the file is what the UI shows, so
/// nothing is lost by making the file name dull.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "preset".to_string()
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn io(err: std::io::Error) -> PresetError {
    PresetError::Io(err.to_string())
}

fn read(dir: &Path, name: &str) -> Result<Preset, PresetError> {
    let path = dir.join(format!("{}.json", slug(name)));
    let text = std::fs::read_to_string(&path).map_err(|_| PresetError::NotFound(name.to_string()))?;
    serde_json::from_str(&text).map_err(|err| PresetError::Json(err.to_string()))
}

/// Every preset in a folder, by name.
pub fn list(dir: &Path) -> Result<Vec<Preset>, PresetError> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // No folder yet is no presets, not a failure.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => return Err(io(err)),
    };
    for entry in entries {
        let entry = entry.map_err(io)?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => continue,
        };
        if let Ok(preset) = serde_json::from_str::<Preset>(&text) {
            out.push(preset);
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// Write a preset, refusing to replace one of the same name.
pub fn save(dir: &Path, preset: &Preset) -> Result<(), PresetError> {
    std::fs::create_dir_all(dir).map_err(io)?;
    let path: PathBuf = dir.join(format!("{}.json", slug(&preset.name)));
    if path.exists() {
        return Err(PresetError::Exists(preset.name.clone()));
    }
    let text = serde_json::to_string_pretty(preset).map_err(|err| PresetError::Json(err.to_string()))?;
    std::fs::write(&path, text).map_err(io)
}

/// Replace a preset of the same name in place, for an explicit save.
pub fn overwrite(dir: &Path, preset: &Preset) -> Result<(), PresetError> {
    std::fs::create_dir_all(dir).map_err(io)?;
    let path = dir.join(format!("{}.json", slug(&preset.name)));
    let text = serde_json::to_string_pretty(preset).map_err(|err| PresetError::Json(err.to_string()))?;
    std::fs::write(&path, text).map_err(io)
}

pub fn rename(dir: &Path, from: &str, to: &str) -> Result<Preset, PresetError> {
    let mut preset = read(dir, from)?;
    let to = to.trim();
    if to.is_empty() {
        return Err(PresetError::BlankName);
    }
    let target = dir.join(format!("{}.json", slug(to)));
    if target.exists() {
        return Err(PresetError::Exists(to.to_string()));
    }
    let _ = std::fs::remove_file(dir.join(format!("{}.json", slug(from))));
    preset.name = to.to_string();
    let text = serde_json::to_string_pretty(&preset).map_err(|err| PresetError::Json(err.to_string()))?;
    std::fs::write(&target, text).map_err(io)?;
    Ok(preset)
}

pub fn delete(dir: &Path, name: &str) -> Result<(), PresetError> {
    let path = dir.join(format!("{}.json", slug(name)));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Err(PresetError::NotFound(name.to_string()))
        }
        Err(err) => Err(io(err)),
    }
}

/// Write a preset somewhere the user chose, as the same JSON the folder holds.
pub fn export(dir: &Path, name: &str, to: &Path) -> Result<(), PresetError> {
    let preset = read(dir, name)?;
    let text = serde_json::to_string_pretty(&preset).map_err(|err| PresetError::Json(err.to_string()))?;
    std::fs::write(to, text).map_err(io)
}

/// Read a preset file and keep it in the folder under its own name, with a
/// suffix if that name is taken — an import must not silently overwrite work.
pub fn import(dir: &Path, from: &Path) -> Result<Preset, PresetError> {
    let text = std::fs::read_to_string(from).map_err(io)?;
    let mut preset: Preset =
        serde_json::from_str(&text).map_err(|err| PresetError::Json(err.to_string()))?;
    preset.name = preset.name.trim().to_string();
    if preset.name.is_empty() {
        return Err(PresetError::BlankName);
    }
    std::fs::create_dir_all(dir).map_err(io)?;
    let mut attempt = preset.clone();
    let mut counter = 2;
    while dir
        .join(format!("{}.json", slug(&attempt.name)))
        .exists()
    {
        attempt.name = format!("{} {}", preset.name, counter);
        counter += 1;
        if counter > 99 {
            return Err(PresetError::Exists(preset.name));
        }
    }
    let text = serde_json::to_string_pretty(&attempt).map_err(|err| PresetError::Json(err.to_string()))?;
    std::fs::write(dir.join(format!("{}.json", slug(&attempt.name))), text).map_err(io)?;
    Ok(attempt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Preset {
        let mut values = BTreeMap::new();
        values.insert("wall_line_count".to_string(), "3".to_string());
        values.insert("layer_height".to_string(), "0.16".to_string());
        Preset::new("Fine walls", values).expect("a name and two values")
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shale-preset-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    #[test]
    fn a_preset_round_trips_through_the_folder() {
        let dir = scratch("round");
        save(&dir, &sample()).expect("save");
        let back = list(&dir).expect("list");
        assert_eq!(back.len(), 1);
        assert_eq!(back[0], sample());
        delete(&dir, "Fine walls").expect("delete");
        assert!(list(&dir).expect("list").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_that_is_taken_is_refused_rather_than_replaced() {
        let dir = scratch("dup");
        save(&dir, &sample()).expect("save");
        match save(&dir, &sample()) {
            Err(PresetError::Exists(name)) => assert_eq!(name, "Fine walls"),
            other => panic!("expected Exists, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_key_is_refused_and_named() {
        let mut values = BTreeMap::new();
        values.insert("wall_line_count".to_string(), "3".to_string());
        values.insert("not_a_cura_setting".to_string(), "1".to_string());
        let preset = Preset::new("Bad", values).expect("a name");
        let known = ["wall_line_count", "layer_height"];
        match preset.check(&known) {
            Err(PresetError::UnknownKey(key)) => assert_eq!(key, "not_a_cura_setting"),
            other => panic!("expected UnknownKey, got {other:?}"),
        }
        // And the sentence names it, so an error dialog can be honest.
        let sentence = PresetError::UnknownKey("not_a_cura_setting".to_string()).to_string();
        assert!(sentence.contains("not_a_cura_setting"), "{sentence}");
        assert!(sentence.contains("would change nothing"), "{sentence}");
    }

    #[test]
    fn the_diff_is_only_what_would_change() {
        let preset = sample();
        let mut current = BTreeMap::new();
        current.insert("wall_line_count".to_string(), "2".to_string());
        current.insert("layer_height".to_string(), "0.16".to_string());
        let diff = preset.diff(&current);
        assert_eq!(diff.len(), 1, "layer_height already matches: {diff:?}");
        assert_eq!(diff[0].0, "wall_line_count");
        assert_eq!(diff[0].1.as_deref(), Some("2"));
        assert_eq!(diff[0].2, "3");
    }

    #[test]
    fn renaming_keeps_the_values_and_moves_the_file() {
        let dir = scratch("rename");
        save(&dir, &sample()).expect("save");
        let renamed = rename(&dir, "Fine walls", "Fine walls 2").expect("rename");
        assert_eq!(renamed.name, "Fine walls 2");
        assert_eq!(renamed.values, sample().values);
        let names: Vec<String> = list(&dir).expect("list").into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Fine walls 2".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn importing_a_taken_name_gets_a_suffix_instead_of_overwriting() {
        let dir = scratch("import");
        save(&dir, &sample()).expect("save");
        let file = dir.join("imported.shale-preset");
        std::fs::write(&file, serde_json::to_string(&sample()).expect("json")).expect("write");
        let imported = import(&dir, &file).expect("import");
        assert_eq!(imported.name, "Fine walls 2");
        assert_eq!(list(&dir).expect("list").len(), 2, "both survive");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_blank_name_is_refused() {
        let mut values = BTreeMap::new();
        values.insert("layer_height".to_string(), "0.2".to_string());
        assert_eq!(Preset::new("   ", values), Err(PresetError::BlankName));
    }

    #[test]
    fn a_slug_cannot_leave_its_folder() {
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("Fine walls"), "fine-walls");
        assert_eq!(slug(""), "preset");
    }
}
