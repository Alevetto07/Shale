//! The choices on Bambu Studio's send dialog, and the `project_file` command
//! that carries them.
//!
//! Studio's dialog is `SelectMachine.cpp` (the options are timelapse, bed
//! levelling, flow dynamics, and use AMS) and `PrintJob.cpp` (the values that
//! go on the command). Vibration calibration is a command field Studio sends
//! as false; it is offered here only for a machine that can run it. Defaults
//! follow Studio's rule: a saved value if the printer offers it, otherwise
//! "auto" when the printer offers automatic mode, otherwise on. A checkbox
//! cannot say "auto", so an automatic machine starts ticked and the command
//! sends Studio's automatic values (`bed_leveling` false and
//! `auto_bed_leveling` 2) rather than a forced full calibration.

use serde::{Deserialize, Serialize};

/// What the send dialog shows, and what the command is built from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintOptions {
    pub timelapse: bool,
    pub bed_levelling: bool,
    pub flow_cali: bool,
    /// False on a P1, which Studio hides because that series has no pressure
    /// advance calibration.
    pub flow_available: bool,
    pub vibration_cali: bool,
    pub vibration_available: bool,
    /// Studio's use-AMS switch starts on. The command is still false when
    /// every tool is on the external spool.
    pub use_ams: bool,
    /// The printer offers Studio's automatic bed check (P2, H2, X2).
    pub automatic_bed: bool,
    /// The printer offers automatic flow calibration.
    pub automatic_flow: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        defaults_for("")
    }
}

/// Studio's starting choices for a printer definition id such as `bambu_lab_a1`.
pub fn defaults_for(printer_id: &str) -> PrintOptions {
    let id = printer_id.to_ascii_lowercase();
    let automatic = contains_any(
        &id,
        &["p2s", "h2d", "h2s", "h2c", "x2d", "n7", "o1d", "o1s", "o1c", "n6"],
    );
    let p1 = contains_any(&id, &["p1p", "p1s", "c11", "c12"]) && !id.contains("p2");
    let known = contains_any(
        &id,
        &[
            "bambu", "a1", "p1", "p2", "x1", "x2", "h2", "n1", "n2", "c11", "c12", "c13",
        ],
    );
    PrintOptions {
        // An A1 is an i3, and Studio forces timelapse off for that architecture.
        // The other models start off too: the switch is not on until someone
        // asks for a recording.
        timelapse: false,
        bed_levelling: true,
        flow_cali: !p1,
        flow_available: known && !p1,
        vibration_cali: false,
        vibration_available: known,
        use_ams: true,
        automatic_bed: automatic,
        automatic_flow: automatic,
    }
}

/// Bambu's `printer_model_id` for a Shale printer id. A1 is `N2S`.
pub fn model_id_for(printer_id: &str) -> &'static str {
    let id = printer_id.to_ascii_lowercase();
    if id.contains("a1_mini") || id.contains("a1mini") || id.contains("_n1") {
        "N1"
    } else if id.contains("x1e") || id.contains("c13") {
        "C13"
    } else if id.contains("x1_carbon") || id.contains("x1c") {
        "3DPrinter-X1-Carbon"
    } else if id.contains("x1") {
        "3DPrinter-X1"
    } else if id.contains("p1s") || id.contains("c12") {
        "C12"
    } else if id.contains("p1p") || id.contains("c11") {
        "C11"
    } else if id.contains("p2s") || id.contains("n7") {
        "N7"
    } else if id.contains("h2d") || id.contains("o1d") {
        "O1D"
    } else if id.contains("h2s") || id.contains("o1s") {
        "O1S"
    } else if id.contains("h2c") || id.contains("o1c") {
        "O1C"
    } else if id.contains("x2") || id.contains("n6") {
        "N6"
    } else {
        "N2S"
    }
}

fn contains_any(id: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| id.contains(needle))
}

/// Fill any choice the dialog did not send with this printer's Studio default.
pub fn resolve(printer_id: &str, chosen: &PartialOptions) -> PrintOptions {
    let defaults = defaults_for(printer_id);
    PrintOptions {
        timelapse: chosen.timelapse.unwrap_or(defaults.timelapse),
        bed_levelling: chosen.bed_levelling.unwrap_or(defaults.bed_levelling),
        flow_cali: chosen.flow_cali.unwrap_or(defaults.flow_cali),
        flow_available: defaults.flow_available,
        vibration_cali: chosen.vibration_cali.unwrap_or(defaults.vibration_cali),
        vibration_available: defaults.vibration_available,
        use_ams: chosen.use_ams.unwrap_or(defaults.use_ams),
        automatic_bed: defaults.automatic_bed,
        automatic_flow: defaults.automatic_flow,
    }
}

/// The fields a dialog may omit. Missing means "use the printer's default".
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PartialOptions {
    pub timelapse: Option<bool>,
    pub bed_levelling: Option<bool>,
    pub flow_cali: Option<bool>,
    pub vibration_cali: Option<bool>,
    pub use_ams: Option<bool>,
}

/// `use_ams` is the switch and the map together. A ticked switch with every
/// tool on the external spool still prints from that spool.
pub fn use_ams(options: &PrintOptions, ams_mapping: &[i32]) -> bool {
    options.use_ams && ams_mapping.iter().any(|slot| *slot >= 0)
}

/// The JSON a Bambu machine takes to start a sliced 3MF that is already there.
///
/// `param` is the plate inside the archive (`Metadata/plate_1.gcode`).
/// `ams_mapping` is always present, which is the same array written into the
/// file. `url` is the FTP path, with the leading slash the printer expects.
pub fn project_file_command(
    sequence: u64,
    remote: &str,
    plate_gcode: &str,
    bed_type: &str,
    options: &PrintOptions,
    ams_mapping: &[i32],
    subtask_name: &str,
) -> serde_json::Value {
    let remote = if remote.starts_with('/') {
        remote.to_string()
    } else {
        format!("/{remote}")
    };
    let bed_on = options.bed_levelling && !options.automatic_bed;
    let flow_on = options.flow_available && options.flow_cali && !options.automatic_flow;
    let mut print = serde_json::json!({
        "sequence_id": sequence.to_string(),
        "command": "project_file",
        "param": plate_gcode,
        "project_id": "0",
        "profile_id": "0",
        "task_id": "0",
        "subtask_id": "0",
        "subtask_name": subtask_name,
        "url": format!("ftp://{remote}"),
        "bed_type": bed_type,
        "timelapse": options.timelapse,
        "bed_leveling": bed_on,
        "bed_levelling": bed_on,
        "flow_cali": flow_on,
        "vibration_cali": options.vibration_available && options.vibration_cali,
        "layer_inspect": true,
        "use_ams": use_ams(options, ams_mapping),
        "ams_mapping": ams_mapping,
    });
    if options.automatic_bed {
        print["auto_bed_leveling"] = serde_json::json!(if options.bed_levelling { 2 } else { 0 });
    }
    if options.automatic_flow && options.flow_available {
        print["auto_flow_cali"] = serde_json::json!(if options.flow_cali { 2 } else { 0 });
    }
    serde_json::json!({ "print": print })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_a1_starts_the_way_studio_does() {
        let options = defaults_for("bambu_lab_a1");
        assert!(!options.timelapse);
        assert!(options.bed_levelling);
        assert!(options.flow_cali);
        assert!(options.flow_available);
        assert!(!options.vibration_cali);
        assert!(options.vibration_available);
        assert!(options.use_ams);
        assert!(!options.automatic_bed);
        assert_eq!(model_id_for("bambu_lab_a1"), "N2S");
        assert_eq!(model_id_for("bambu_lab_a1_mini"), "N1");
    }

    #[test]
    fn a_p1_hides_flow_and_a_p2s_sends_automatic_calibration() {
        let p1 = defaults_for("bambu_lab_p1s");
        assert!(!p1.flow_available);
        assert!(!p1.flow_cali);
        assert!(!p1.automatic_bed);

        let p2 = defaults_for("bambu_lab_p2s");
        assert!(p2.automatic_bed);
        assert!(p2.automatic_flow);
        assert!(p2.bed_levelling);
        let command = project_file_command(
            3,
            "/cache/plate.gcode.3mf",
            "Metadata/plate_1.gcode",
            "textured_plate",
            &p2,
            &[0],
            "plate",
        );
        assert_eq!(command["print"]["command"], "project_file");
        assert_eq!(command["print"]["bed_leveling"], false);
        assert_eq!(command["print"]["auto_bed_leveling"], 2);
        assert_eq!(command["print"]["flow_cali"], false);
        assert_eq!(command["print"]["auto_flow_cali"], 2);
        assert_eq!(command["print"]["use_ams"], true);
        assert_eq!(command["print"]["ams_mapping"], serde_json::json!([0]));
        assert_eq!(command["print"]["param"], "Metadata/plate_1.gcode");
    }

    #[test]
    fn an_unticked_ams_switch_keeps_the_map_and_does_not_use_it() {
        let mut options = defaults_for("bambu_lab_x1_carbon");
        options.use_ams = false;
        assert!(!use_ams(&options, &[1, -1]));
        let command = project_file_command(
            1,
            "/cache/x.gcode.3mf",
            "Metadata/plate_2.gcode",
            "smooth_plate",
            &options,
            &[1, -1],
            "x",
        );
        assert_eq!(command["print"]["use_ams"], false);
        assert_eq!(command["print"]["ams_mapping"], serde_json::json!([1, -1]));
        assert_eq!(model_id_for("bambu_lab_x1_carbon"), "3DPrinter-X1-Carbon");
    }
}
