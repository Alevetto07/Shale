//! Uranium instance containers (`.inst.cfg`): variants, qualities, and intents.
//!
//! A value in the `[values]` section is a literal unless it begins with `=`,
//! in which case the remainder is a formula. That prefix is how Cura stores
//! expressions without a Python parser on every line.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::Error;

#[derive(Clone, Debug)]
pub struct InstanceFile {
    pub name: String,
    pub definition: String,
    pub type_name: String,
    pub quality_type: Option<String>,
    pub variant: Option<String>,
    pub material: Option<String>,
    pub global_quality: bool,
    pub weight: i32,
    pub intent_category: Option<String>,
    /// Raw `[values]` entries. A leading `=` marks a formula.
    pub values: HashMap<String, String>,
}

pub fn load_instance(path: &Path) -> Result<InstanceFile, Error> {
    let text = fs::read_to_string(path)?;
    let mut section = String::new();
    let mut name = String::new();
    let mut definition = String::new();
    let mut type_name = String::new();
    let mut quality_type = None;
    let mut variant = None;
    let mut material = None;
    let mut global_quality = false;
    let mut weight = 0;
    let mut intent_category = None;
    let mut values = HashMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match section.as_str() {
            "general" => match key {
                "name" => name = value.to_string(),
                "definition" => definition = value.to_string(),
                _ => {}
            },
            "metadata" => match key {
                "type" => type_name = value.to_string(),
                "quality_type" => quality_type = Some(value.to_string()),
                "variant" => variant = Some(value.to_string()),
                "material" => material = Some(value.to_string()),
                "global_quality" => {
                    global_quality = value.eq_ignore_ascii_case("true");
                }
                "weight" => weight = value.parse().unwrap_or(0),
                "intent_category" => intent_category = Some(value.to_string()),
                _ => {}
            },
            "values" => {
                values.insert(key.to_string(), value.to_string());
            }
            _ => {}
        }
    }

    if definition.is_empty() {
        return Err(Error::message(format!(
            "{} has no definition id",
            path.display()
        )));
    }
    Ok(InstanceFile {
        name,
        definition,
        type_name,
        quality_type,
        variant,
        material,
        global_quality,
        weight,
        intent_category,
        values,
    })
}
