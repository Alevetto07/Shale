//! Cura machine and extruder definitions (`*.def.json`).
//!
//! A definition is a tree of setting groups. Concrete machines usually inherit
//! `fdmprinter` and only ship an `overrides` object. Shale flattens that tree
//! and merges it later, root definition first.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value as Json;

use crate::error::Error;

#[derive(Clone, Debug, Default)]
pub struct RawSetting {
    pub ty: Option<String>,
    pub default_value: Option<Json>,
    pub value: Option<Json>,
    pub resolve: Option<Json>,
    pub label: Option<String>,
    pub description: Option<String>,
}

impl RawSetting {
    pub fn merge(&mut self, other: &Self) {
        if other.ty.is_some() {
            self.ty.clone_from(&other.ty);
        }
        if other.default_value.is_some() {
            self.default_value.clone_from(&other.default_value);
        }
        if other.value.is_some() {
            self.value.clone_from(&other.value);
        }
        if other.resolve.is_some() {
            self.resolve.clone_from(&other.resolve);
        }
        if other.label.is_some() {
            self.label.clone_from(&other.label);
        }
        if other.description.is_some() {
            self.description.clone_from(&other.description);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Definition {
    pub id: String,
    pub name: String,
    pub inherits: Option<String>,
    pub visible: bool,
    pub manufacturer: String,
    pub quality_definition: Option<String>,
    pub preferred_quality_type: Option<String>,
    pub preferred_variant_name: Option<String>,
    pub preferred_material: Option<String>,
    pub extruder_trains: Vec<(i32, String)>,
    pub is_extruder: bool,
    pub settings: HashMap<String, RawSetting>,
}

pub fn load_definition(path: &Path, is_extruder: bool) -> Result<Definition, Error> {
    let text = fs::read_to_string(path)?;
    let json: Json = serde_json::from_str(&text).map_err(|source| Error::Json {
        path: path.display().to_string(),
        source,
    })?;
    let id = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".def.json"))
        .unwrap_or("unknown")
        .to_string();
    let obj = json
        .as_object()
        .ok_or_else(|| Error::message(format!("{} is not a JSON object", path.display())))?;
    let name = obj
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or(&id)
        .to_string();
    let inherits = obj
        .get("inherits")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let metadata = obj.get("metadata").and_then(|value| value.as_object());
    let visible = metadata
        .and_then(|meta| meta.get("visible"))
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let manufacturer = metadata
        .and_then(|meta| meta.get("manufacturer"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let quality_definition = metadata
        .and_then(|meta| meta.get("quality_definition"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let preferred_quality_type = metadata
        .and_then(|meta| meta.get("preferred_quality_type"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let preferred_variant_name = metadata
        .and_then(|meta| meta.get("preferred_variant_name"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let preferred_material = metadata
        .and_then(|meta| meta.get("preferred_material"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let mut extruder_trains = Vec::new();
    if let Some(trains) = metadata
        .and_then(|meta| meta.get("machine_extruder_trains"))
        .and_then(|value| value.as_object())
    {
        for (index, id) in trains {
            if let (Ok(index), Some(id)) = (index.parse::<i32>(), id.as_str()) {
                extruder_trains.push((index, id.to_string()));
            }
        }
        extruder_trains.sort_by_key(|(index, _)| *index);
    }
    let metadata_type = metadata
        .and_then(|meta| meta.get("type"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let mut settings = HashMap::new();
    if let Some(groups) = obj.get("settings").and_then(|value| value.as_object()) {
        for (key, group) in groups {
            collect_setting(key, group, &mut settings);
        }
    }
    if let Some(overrides) = obj.get("overrides").and_then(|value| value.as_object()) {
        for (key, override_value) in overrides {
            let incoming = raw_from(override_value);
            settings.entry(key.clone()).or_default().merge(&incoming);
        }
    }
    Ok(Definition {
        id,
        name,
        inherits,
        visible,
        manufacturer,
        quality_definition,
        preferred_quality_type,
        preferred_variant_name,
        preferred_material,
        extruder_trains,
        is_extruder: is_extruder || metadata_type == "extruder",
        settings,
    })
}

fn collect_setting(key: &str, node: &Json, into: &mut HashMap<String, RawSetting>) {
    let Some(obj) = node.as_object() else {
        return;
    };
    if let Some(children) = obj.get("children").and_then(|value| value.as_object()) {
        for (child_key, child) in children {
            collect_setting(child_key, child, into);
        }
    }
    if obj.contains_key("default_value")
        || obj.contains_key("value")
        || obj.contains_key("resolve")
        || obj.contains_key("type")
    {
        // Categories carry a type sometimes and no value. Keep them only when
        // they actually define a value the engine might read.
        if obj.contains_key("default_value")
            || obj.contains_key("value")
            || obj.contains_key("resolve")
        {
            into.insert(key.to_string(), raw_from(node));
        }
    }
}

fn raw_from(node: &Json) -> RawSetting {
    let Some(obj) = node.as_object() else {
        return RawSetting::default();
    };
    RawSetting {
        ty: obj
            .get("type")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        default_value: obj.get("default_value").cloned(),
        value: obj.get("value").cloned(),
        resolve: obj.get("resolve").filter(|value| !value.is_null()).cloned(),
        label: obj
            .get("label")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        description: obj
            .get("description")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }
}
