//! Suggestions after an import, a move, a setting change, or a slice.
//!
//! Rules are pure: they read a [`Scene`] and return suggestions. Nothing here
//! slices, and nothing here touches a printer. Dismissals are applied by
//! [`evaluate`] so a dismissed id never comes back.

mod ams;
mod bridge;
mod brim;
mod filament;
mod gcode;
mod geom;
mod height;
mod interface;
mod moisture;
mod nonplanar;
mod orient;
mod packing;
mod purge;
mod seam;
mod strength;
mod thin;
mod time_sink;
mod warp;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::mesh::{bake, Transform, TriangleMesh};

pub use gcode::{facts as gcode_facts, GcodeFacts};

/// How many suggestions Prepare shows at once.
pub const VISIBLE_LIMIT: usize = 3;

fn loaded_by_default() -> bool {
    true
}

/// One spool, as a caller describes it. The type may be a profile name;
/// [`filament_kind`] reads PLA, PETG and the rest out of it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SlotQuery {
    pub index: usize,
    pub filament_type: String,
    #[serde(default)]
    pub colour: String,
    #[serde(default)]
    pub material_id: String,
    #[serde(default = "loaded_by_default")]
    pub loaded: bool,
}

impl SlotQuery {
    pub fn into_slot(self) -> FilamentSlot {
        FilamentSlot {
            index: self.index,
            material_id: self.material_id,
            filament_type: self.filament_type,
            colour: self.colour,
            loaded: self.loaded,
        }
    }
}

/// What a caller adds on top of the loaded meshes.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SuggestQuery {
    #[serde(default)]
    pub slots: Vec<SlotQuery>,
    #[serde(default)]
    pub device_slots: Option<Vec<SlotQuery>>,
    pub humidity_percent: Option<f64>,
    pub hours_since_dry: Option<f64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dismissed: Vec<String>,
    #[serde(default)]
    pub global_dismissed: Vec<String>,
    #[serde(default)]
    pub shown_once: Vec<String>,
    pub nonplanar_mode: Option<String>,
    pub multi_material: Option<bool>,
    /// `None` returns the ranked list. Prepare asks for [`VISIBLE_LIMIT`].
    pub limit: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Tip,
    Warning,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SuggestionFix {
    pub label: String,
    /// Plate-wide setting keys, as the engine expects them.
    pub settings: BTreeMap<String, String>,
    /// Per-object setting keys, keyed by the object's id.
    pub object_settings: BTreeMap<String, BTreeMap<String, String>>,
    /// Replacement rotation in degrees, keyed by object id.
    pub rotations_deg: BTreeMap<String, [f64; 3]>,
    /// AMS slot index, keyed by object id.
    pub slots: BTreeMap<String, usize>,
    /// Destination plate index, keyed by object id.
    pub plates: BTreeMap<String, usize>,
    pub nonplanar_mode: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: String,
    pub rule_id: String,
    pub severity: Severity,
    pub title: String,
    pub reason: String,
    pub objects: Vec<String>,
    pub fix: Option<SuggestionFix>,
    pub dismissible: bool,
    /// Higher sorts first within the same severity.
    pub score: i32,
    /// Show at most once per project, even without an explicit dismiss.
    pub once: bool,
}

#[derive(Clone, Debug)]
pub struct SceneObject {
    pub id: String,
    pub name: String,
    pub mesh: TriangleMesh,
    pub transform: Transform,
    pub plate: usize,
    pub settings: BTreeMap<String, String>,
    pub slot: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct PrinterFacts {
    pub id: String,
    pub name: String,
    pub bed_width_mm: f64,
    pub bed_depth_mm: f64,
    pub bed_height_mm: f64,
    pub nozzle_mm: f64,
    pub line_width_mm: f64,
    pub speed_mm_s: f64,
    pub bed_temperature_c: f64,
    pub enclosed: bool,
    pub multi_material: bool,
}

#[derive(Clone, Debug)]
pub struct FilamentSlot {
    pub index: usize,
    pub material_id: String,
    pub filament_type: String,
    pub colour: String,
    pub loaded: bool,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub objects: Vec<SceneObject>,
    pub printer: PrinterFacts,
    /// Filaments the project is using. Slot 0 is the plate material.
    pub slots: Vec<FilamentSlot>,
    /// What the printer actually has loaded. `None` when it is not connected.
    pub device_slots: Option<Vec<FilamentSlot>>,
    pub humidity_percent: Option<f64>,
    pub hours_since_dry: Option<f64>,
    pub settings: BTreeMap<String, String>,
    pub gcode: Option<GcodeFacts>,
    pub nonplanar_mode: String,
    pub tags: Vec<String>,
    pub dismissed: Vec<String>,
    pub global_dismissed: Vec<String>,
    pub shown_once: Vec<String>,
}

/// Run every rule, drop dismissals and duplicates, and rank what is left.
pub fn evaluate(scene: &Scene) -> Vec<Suggestion> {
    let mut found = Vec::new();
    found.extend(brim::check(scene));
    found.extend(interface::check(scene));
    found.extend(orient::check(scene));
    found.extend(seam::check(scene));
    found.extend(thin::check(scene));
    found.extend(filament::check(scene));
    found.extend(moisture::check(scene));
    found.extend(bridge::check(scene));
    found.extend(warp::check(scene));
    found.extend(packing::check(scene));
    found.extend(strength::check(scene));
    found.extend(purge::check(scene));
    found.extend(time_sink::check(scene));
    found.extend(height::check(scene));
    found.extend(ams::check(scene));
    found.extend(nonplanar::check(scene));
    rank(scene, found)
}

/// The first `limit` suggestions Prepare should show.
pub fn present(scene: &Scene, limit: usize) -> Vec<Suggestion> {
    evaluate(scene).into_iter().take(limit).collect()
}

fn suggestion_family(id: &str) -> &str {
    match id.rfind(':') {
        Some(at) => &id[..at],
        None => id,
    }
}

fn rank(scene: &Scene, mut found: Vec<Suggestion>) -> Vec<Suggestion> {
    let dismissed: BTreeSet<&str> = scene
        .dismissed
        .iter()
        .chain(scene.global_dismissed.iter())
        .map(String::as_str)
        .collect();
    let shown: BTreeSet<&str> = scene.shown_once.iter().map(String::as_str).collect();
    found.retain(|item| {
        if item.once && (shown.contains(item.rule_id.as_str()) || shown.contains(item.id.as_str()))
        {
            return false;
        }
        !dismissed.contains(item.id.as_str()) && !dismissed.contains(item.rule_id.as_str())
    });
    let mut seen = BTreeSet::new();
    found.retain(|item| seen.insert(item.id.clone()));
    // One suggestion per kind. A kit of similar parts should not repeat
    // "adaptive layers" once for every object; the strongest one stands for them.
    let mut best: BTreeMap<String, usize> = BTreeMap::new();
    for (index, item) in found.iter().enumerate() {
        let family = suggestion_family(&item.id);
        match best.get(family) {
            Some(previous) if found[*previous].score >= item.score => {}
            _ => {
                best.insert(family.to_string(), index);
            }
        }
    }
    let keep: BTreeSet<usize> = best.into_values().collect();
    let mut index = 0;
    found.retain(|_| {
        let keep_this = keep.contains(&index);
        index += 1;
        keep_this
    });
    found.sort_by(|a, b| {
        let rank = |item: &Suggestion| {
            (
                match item.severity {
                    Severity::Warning => 0,
                    Severity::Tip => 1,
                },
                -item.score,
            )
        };
        rank(a).cmp(&rank(b)).then_with(|| a.id.cmp(&b.id))
    });
    found
}

pub fn filament_kind(label: &str) -> String {
    let upper = label.to_ascii_uppercase();
    const ORDER: &[&str] = &[
        "PETG", "TPU", "ABS", "ASA", "PVA", "NYLON", "PA", "PC", "PLA", "HIPS",
    ];
    for kind in ORDER {
        if upper.contains(kind) {
            return if *kind == "NYLON" {
                "PA".to_string()
            } else {
                (*kind).to_string()
            };
        }
    }
    "UNKNOWN".to_string()
}

pub fn printer_is_enclosed(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    id.contains("x1") || id.contains("p1s") || id.contains("p2s")
}

pub fn read_global_dismissals(dir: &Path) -> Vec<String> {
    let path = dir.join("suggestion-dismissals.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn remember_global_dismissal(dir: &Path, rule_id: &str) -> Result<(), String> {
    let mut ids = read_global_dismissals(dir);
    if !ids.iter().any(|id| id == rule_id) {
        ids.push(rule_id.to_string());
    }
    let path = dir.join("suggestion-dismissals.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let text = serde_json::to_string_pretty(&ids).map_err(|err| err.to_string())?;
    std::fs::write(path, text).map_err(|err| err.to_string())
}

pub(crate) fn setting<'a>(scene: &'a Scene, object: &'a SceneObject, key: &str) -> Option<&'a str> {
    object
        .settings
        .get(key)
        .map(String::as_str)
        .or_else(|| scene.settings.get(key).map(String::as_str))
}

pub(crate) fn is_on(value: Option<&str>) -> bool {
    matches!(
        value
            .map(|text| text.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

pub(crate) fn number(value: Option<&str>) -> Option<f64> {
    value.and_then(|text| text.trim().parse().ok())
}

pub(crate) fn placed(object: &SceneObject) -> TriangleMesh {
    bake(&object.mesh, &object.transform)
}

pub(crate) fn object_slot<'a>(scene: &'a Scene, object: &SceneObject) -> Option<&'a FilamentSlot> {
    let index = object.slot.unwrap_or(0);
    scene
        .slots
        .iter()
        .find(|slot| slot.index == index && slot.loaded)
}

pub(crate) fn tip(
    rule_id: &str,
    id: String,
    title: impl Into<String>,
    reason: impl Into<String>,
    objects: Vec<String>,
    fix: Option<SuggestionFix>,
    score: i32,
) -> Suggestion {
    Suggestion {
        id,
        rule_id: rule_id.to_string(),
        severity: Severity::Tip,
        title: title.into(),
        reason: reason.into(),
        objects,
        fix,
        dismissible: true,
        score,
        once: false,
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub fn object(id: &str, mesh: TriangleMesh) -> SceneObject {
        SceneObject {
            id: id.to_string(),
            name: id.to_string(),
            mesh,
            transform: Transform::default(),
            plate: 0,
            settings: BTreeMap::new(),
            slot: None,
        }
    }

    pub fn scene(objects: Vec<SceneObject>) -> Scene {
        Scene {
            objects,
            printer: PrinterFacts {
                id: "bambu_lab_a1".into(),
                name: "Bambu Lab A1".into(),
                bed_width_mm: 256.0,
                bed_depth_mm: 256.0,
                bed_height_mm: 256.0,
                nozzle_mm: 0.4,
                line_width_mm: 0.42,
                speed_mm_s: 200.0,
                bed_temperature_c: 60.0,
                enclosed: false,
                multi_material: false,
            },
            slots: vec![FilamentSlot {
                index: 0,
                material_id: "pla".into(),
                filament_type: "PLA".into(),
                colour: "#ffffff".into(),
                loaded: true,
            }],
            device_slots: None,
            humidity_percent: None,
            hours_since_dry: None,
            settings: BTreeMap::new(),
            gcode: None,
            nonplanar_mode: "off".into(),
            tags: Vec::new(),
            dismissed: Vec::new(),
            global_dismissed: Vec::new(),
            shown_once: Vec::new(),
        }
    }
}
