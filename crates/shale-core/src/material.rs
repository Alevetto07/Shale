//! fdm_materials XML profiles (CC0).
//!
//! Cura maps the human-readable keys in these files onto setting ids. Shale
//! understands the handful that change a normal single-extruder slice:
//! temperatures, fan, retraction, and diameter. Machine-specific blocks
//! override the generic numbers when their `product` matches the printer.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::Error;

#[derive(Clone, Debug)]
pub struct Material {
    pub id: String,
    pub brand: String,
    pub name: String,
    pub color: String,
    pub diameter: f64,
    /// `PLA`, `PETG`, … for a filament from an Orca-family library, which is
    /// the only place the type is written down. Cura's materials never say.
    pub filament_type: Option<String>,
    pub settings: HashMap<String, f64>,
    pub machines: Vec<MachineMaterial>,
}

#[derive(Clone, Debug, Default)]
pub struct MachineMaterial {
    pub products: Vec<String>,
    pub settings: HashMap<String, f64>,
    pub hotends: Vec<HotendMaterial>,
}

#[derive(Clone, Debug, Default)]
pub struct HotendMaterial {
    pub id: String,
    pub settings: HashMap<String, f64>,
}

pub fn load_material(path: &Path) -> Result<Material, Error> {
    let text = fs::read_to_string(path)?;
    let id = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("material")
        .trim_end_matches(".xml.fdm_material")
        .trim_end_matches(".fdm_material")
        .to_string();
    parse_material(&id, &text).map_err(|err| Error::message(format!("{}: {err}", path.display())))
}

fn parse_material(id: &str, text: &str) -> Result<Material, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut brand = String::new();
    let mut name = String::new();
    let mut color = String::new();
    let mut diameter = 1.75;
    let mut settings = HashMap::new();
    let mut machines = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut setting_key: Option<String> = None;
    let mut text_buf = String::new();
    let mut machine_idx: Option<usize> = None;
    let mut hotend_idx: Option<usize> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let local = local_name(event.name().as_ref());
                if local == "machine" {
                    machines.push(MachineMaterial::default());
                    machine_idx = Some(machines.len() - 1);
                    hotend_idx = None;
                } else if local == "hotend" {
                    if let Some(index) = machine_idx {
                        let hotend_id = attr(&event, "id").unwrap_or_default();
                        machines[index].hotends.push(HotendMaterial {
                            id: hotend_id,
                            settings: HashMap::new(),
                        });
                        hotend_idx = Some(machines[index].hotends.len() - 1);
                    }
                } else if local == "machine_identifier" {
                    if let Some(product) = attr(&event, "product") {
                        if let Some(index) = machine_idx {
                            machines[index].products.push(product);
                        }
                    }
                } else if local == "setting" {
                    setting_key = attr(&event, "key");
                    text_buf.clear();
                }
                stack.push(local);
            }
            Ok(Event::Empty(event)) => {
                let local = local_name(event.name().as_ref());
                if local == "machine_identifier" {
                    if let Some(product) = attr(&event, "product") {
                        if let Some(index) = machine_idx {
                            machines[index].products.push(product);
                        }
                    }
                } else if local == "setting" {
                    if let Some(key) = attr(&event, "key") {
                        store_setting(
                            &key,
                            "",
                            &stack,
                            machine_idx,
                            hotend_idx,
                            &mut settings,
                            &mut machines,
                        );
                    }
                }
            }
            Ok(Event::Text(event)) => {
                text_buf.push_str(&event.unescape().map_err(|err| err.to_string())?);
            }
            Ok(Event::End(event)) => {
                let local = local_name(event.name().as_ref());
                if local == "setting" {
                    if let Some(key) = setting_key.take() {
                        store_setting(
                            &key,
                            text_buf.trim(),
                            &stack,
                            machine_idx,
                            hotend_idx,
                            &mut settings,
                            &mut machines,
                        );
                    }
                    text_buf.clear();
                } else if local == "brand" && stack.iter().any(|item| item == "name") {
                    brand = text_buf.trim().to_string();
                    text_buf.clear();
                } else if local == "material" && stack.iter().any(|item| item == "name") {
                    name = text_buf.trim().to_string();
                    text_buf.clear();
                } else if local == "color" {
                    color = text_buf.trim().to_string();
                    text_buf.clear();
                } else if local == "diameter" {
                    if let Ok(value) = text_buf.trim().parse::<f64>() {
                        diameter = value;
                    }
                    text_buf.clear();
                } else if local == "density" {
                    // <properties><density> is grams per cubic centimetre, the
                    // number a spool's weight and a print's weight are both
                    // counted in. Cura keeps it here rather than among the
                    // settings, and without it the job card cannot say how much
                    // a print weighs.
                    if let Ok(value) = text_buf.trim().parse::<f64>() {
                        if value > 0.0 {
                            settings.insert("material_density".to_string(), value);
                        }
                    }
                    text_buf.clear();
                } else if local == "hotend" {
                    hotend_idx = None;
                    text_buf.clear();
                } else if local == "machine" {
                    machine_idx = None;
                    hotend_idx = None;
                    text_buf.clear();
                } else {
                    text_buf.clear();
                }
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(err.to_string()),
            _ => {}
        }
        buf.clear();
    }

    Ok(Material {
        id: id.to_string(),
        brand,
        name,
        color,
        diameter,
        filament_type: None,
        settings,
        machines,
    })
}

fn store_setting(
    key: &str,
    raw: &str,
    _stack: &[String],
    machine_idx: Option<usize>,
    hotend_idx: Option<usize>,
    base: &mut HashMap<String, f64>,
    machines: &mut [MachineMaterial],
) {
    let Some(mapped) = map_setting_key(key) else {
        return;
    };
    let Ok(value) = raw.parse::<f64>() else {
        return;
    };
    if let (Some(machine), Some(hotend)) = (machine_idx, hotend_idx) {
        if let Some(machine) = machines.get_mut(machine) {
            if let Some(hotend) = machine.hotends.get_mut(hotend) {
                hotend.settings.insert(mapped.to_string(), value);
            }
        }
        return;
    }
    if let Some(machine) = machine_idx {
        if let Some(machine) = machines.get_mut(machine) {
            machine.settings.insert(mapped.to_string(), value);
        }
        return;
    }
    base.insert(mapped.to_string(), value);
}

fn map_setting_key(key: &str) -> Option<&'static str> {
    Some(match key {
        "print temperature" => "default_material_print_temperature",
        "heated bed temperature" => "default_material_bed_temperature",
        "standby temperature" => "material_standby_temperature",
        "print cooling" => "cool_fan_speed",
        "retraction amount" => "retraction_amount",
        "retraction speed" => "retraction_speed",
        "build volume temperature" => "build_volume_temperature",
        "adhesion tendency" => "material_adhesion_tendency",
        "surface energy" => "material_surface_energy",
        _ => return None,
    })
}

fn local_name(qname: &[u8]) -> String {
    let name = std::str::from_utf8(qname).unwrap_or("");
    name.rsplit(':').next().unwrap_or(name).to_string()
}

fn attr(event: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    for attribute in event.attributes().flatten() {
        if attribute.key.local_name().as_ref() == key.as_bytes() {
            return attribute
                .unescape_value()
                .ok()
                .map(|value| value.into_owned());
        }
    }
    None
}
