//! Bambu HMS codes, as they arrive on MQTT.
//!
//! A report carries `print.hms` as `{attr, code}` pairs. Bambu Studio shows each
//! pair as four hex groups — `0300-0100-0001-0007` — and opens
//! `https://wiki.bambulab.com/en/x1/troubleshooting/hmscode/0300_0100_0001_0007`.
//! That URL is the one the studio builds, including for codes whose wiki page
//! has since moved: the site redirects.
//!
//! The wording here is Shale's, in British English. The wiki page is where the
//! long troubleshooting steps live.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One alert the printer is currently raising.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HmsAlert {
    /// On-screen form, with hyphens: `0300-0100-0001-0007`.
    pub code: String,
    /// The wiki page Bambu Studio opens for this code.
    pub wiki: String,
    /// `error` stops a job, `warning` pauses one, `notice` does not.
    pub severity: String,
    pub message: String,
    /// The spool has run out, or the printer has switched because one did.
    pub runout: bool,
}

const WIKI: &str = "https://wiki.bambulab.com/en/x1/troubleshooting/hmscode";

/// Read the `hms` array from one report. A missing field is not the same as an
/// empty one: callers keep the previous alerts when the key is absent.
pub fn present(print: &Value) -> Option<Vec<HmsAlert>> {
    let array = print.get("hms")?.as_array()?;
    Some(decode_array(array))
}

fn decode_array(array: &[Value]) -> Vec<HmsAlert> {
    let mut alerts = Vec::new();
    for entry in array {
        let Some(attr) = u32_field(entry, "attr") else {
            continue;
        };
        let Some(code) = u32_field(entry, "code") else {
            continue;
        };
        // A zero pair is padding, not an alert. The machine sends them.
        if attr == 0 || code == 0 {
            continue;
        }
        alerts.push(describe(attr, code));
    }
    alerts
}

/// The four groups Bambu prints, and the sentence that goes with them.
pub fn describe(attr: u32, code: u32) -> HmsAlert {
    let groups = [
        (attr >> 16) as u16,
        (attr & 0xFFFF) as u16,
        (code >> 16) as u16,
        (code & 0xFFFF) as u16,
    ];
    let hyphen = format!(
        "{:04X}-{:04X}-{:04X}-{:04X}",
        groups[0], groups[1], groups[2], groups[3]
    );
    let wiki = format!(
        "{WIKI}/{:04X}_{:04X}_{:04X}_{:04X}",
        groups[0], groups[1], groups[2], groups[3]
    );
    let runout = filament_runout(groups);
    let message = message_for(groups).unwrap_or_else(|| fallback(groups, &hyphen));
    HmsAlert {
        code: hyphen,
        wiki,
        severity: severity(groups[2]).to_string(),
        message,
        runout,
    }
}

/// Official wiki levels: `0001` error, `0002` warning, `0003` notice.
fn severity(level: u16) -> &'static str {
    match level {
        1 => "error",
        2 => "warning",
        3 => "notice",
        _ => "alert",
    }
}

/// Slot run-out warnings, the "press Retry" run-out, and the notice that a
/// backup slot took over. The part id is what the wiki groups these by.
fn filament_runout(groups: [u16; 4]) -> bool {
    let part = groups[1];
    let level = groups[2];
    let id = groups[3];
    let slot = (part >> 8) & 0xFF;
    if (0x20..=0x23).contains(&slot) && level == 0x0002 && id == 0x0001 {
        return true;
    }
    if part == 0x7000 && level == 0x0002 && id == 0x0007 {
        return true;
    }
    if (0x20..=0x23).contains(&slot) && level == 0x0003 && id == 0x0002 {
        return true;
    }
    false
}

fn message_for(groups: [u16; 4]) -> Option<String> {
    if filament_runout(groups) {
        return Some(runout_message(groups));
    }
    match groups {
        [0x0300, 0x0100, 0x0001, 0x0003] => {
            Some("The heatbed temperature is abnormal. The heater is over temperature.".into())
        }
        [0x0300, 0x0100, 0x0001, _] => Some("The heatbed temperature is abnormal.".into()),
        [0x0C00, 0x0300, 0x0002, 0x0002] => {
            Some("First-layer inspection stopped because the lidar data was abnormal.".into())
        }
        _ => None,
    }
}

fn runout_message(groups: [u16; 4]) -> String {
    let unit = ams_unit(groups[0]);
    let slot = slot_number(groups[1]);
    if groups[1] == 0x7000 {
        return format!(
            "{unit} has run out of filament. Put a new spool in the same slot and resume the print."
        );
    }
    if groups[2] == 0x0003 {
        return format!(
            "{unit} slot {slot} ran out of filament. The printer switched to another slot with the same filament."
        );
    }
    format!(
        "{unit} slot {slot} has run out of filament. Load a new spool in that slot and resume the print."
    )
}

fn ams_unit(module_word: u16) -> String {
    let kind = match (module_word >> 8) as u8 {
        0x12 => "AMS Lite",
        0x18 => "AMS heater",
        _ => "AMS",
    };
    let index = (module_word & 0xFF) as u32 + 1;
    format!("{kind} {index}")
}

fn slot_number(part: u16) -> u32 {
    let high = (part >> 8) as u32;
    if (0x20..=0x23).contains(&high) {
        high - 0x1F
    } else {
        1
    }
}

fn fallback(groups: [u16; 4], hyphen: &str) -> String {
    let who = match (groups[0] >> 8) as u8 {
        0x03 => "The motion controller",
        0x05 => "The main board",
        0x07 | 0x12 | 0x18 => "The AMS",
        0x08 => "The toolhead",
        0x0C => "The lidar",
        _ => "The printer",
    };
    format!("{who} reported {hyphen}. The Bambu wiki page for this code has the steps.")
}

fn u32_field(entry: &Value, key: &str) -> Option<u32> {
    match entry.get(key)? {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .or_else(|| number.as_i64().and_then(|value| u32::try_from(value).ok())),
        Value::String(text) => {
            let text = text.trim();
            if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                text.parse().ok()
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_heatbed_code_uses_the_studio_wiki_link() {
        // attr 0x03000100, code 0x00010007 — the worked example on the wiki.
        let alert = describe(0x0300_0100, 0x0001_0007);
        assert_eq!(alert.code, "0300-0100-0001-0007");
        assert_eq!(
            alert.wiki,
            "https://wiki.bambulab.com/en/x1/troubleshooting/hmscode/0300_0100_0001_0007"
        );
        assert_eq!(alert.severity, "error");
        assert!(alert.message.contains("heatbed"));
        assert!(!alert.runout);
    }

    #[test]
    fn an_ams_slot_running_out_is_named_in_british_english() {
        let alert = describe(0x0700_2000, 0x0002_0001);
        assert_eq!(alert.code, "0700-2000-0002-0001");
        assert!(alert.wiki.ends_with("/0700_2000_0002_0001"));
        assert_eq!(alert.severity, "warning");
        assert!(alert.runout);
        assert_eq!(
            alert.message,
            "AMS 1 slot 1 has run out of filament. Load a new spool in that slot and resume the print."
        );

        let switched = describe(0x0701_2100, 0x0003_0002);
        assert!(switched.runout);
        assert!(switched.message.contains("AMS 2 slot 2"));
        assert!(switched.message.contains("switched"));

        let retry = describe(0x0700_7000, 0x0002_0007);
        assert!(retry.runout);
        assert!(retry.message.contains("resume the print"));
    }

    #[test]
    fn an_unknown_code_still_names_itself_and_links_the_wiki() {
        let alert = describe(0x0500_0001, 0x0001_0004);
        assert_eq!(alert.code, "0500-0001-0001-0004");
        assert!(alert.wiki.contains("0500_0001_0001_0004"));
        assert!(alert.message.contains("main board"));
        assert!(!alert.runout);
    }

    #[test]
    fn padding_and_a_missing_array_are_not_alerts() {
        let print = json!({ "hms": [
            { "attr": 0, "code": 0 },
            { "attr": 50331904, "code": 65543 },
            { "attr": "0x0C000300", "code": "131074" }
        ]});
        let alerts = present(&print).unwrap();
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].code, "0300-0100-0001-0007");
        assert_eq!(alerts[1].code, "0C00-0300-0002-0002");
        assert!(alerts[1].message.contains("lidar"));
        assert!(present(&json!({})).is_none());
        assert_eq!(present(&json!({ "hms": [] })).unwrap().len(), 0);
    }
}
