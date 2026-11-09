//! Structured print instructions compiled into the engine's G-code.
//!
//! CuraEngine stays unmodified. After it writes a file, Shale inserts a short
//! block at the start of the chosen layer. Marlin is the flavour that is
//! fully spelled out. RepRap and Klipper get the same temperatures, fan, and
//! messages, and a pause command those firmwares actually accept. See the
//! flavour notes on [`GcodeFlavour`].

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Firmware family used when an instruction is turned into G-code.
///
/// `machine_gcode_flavor` from the resolved profile is mapped with
/// [`GcodeFlavour::from_cura`]. Cura's own label for an Ender-3 or a Prusa
/// i3 is `RepRap (Marlin/Sprinter)`, which is Marlin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GcodeFlavour {
    /// `M0` pause, `M600` filament change, `M300` beep, `M117` message.
    Marlin,
    /// `M226` pause (not Marlin's `M0`/`M600`). Temperatures and fan match Marlin.
    RepRap,
    /// `PAUSE` macro for a user pause, `M600` for a filament change when the
    /// printer defines that macro. Temperatures, fan, and `M117` match Marlin.
    Klipper,
}

impl GcodeFlavour {
    pub fn from_cura(value: &str) -> Self {
        let lower = value.to_ascii_lowercase();
        if lower.contains("klipper") {
            Self::Klipper
        } else if lower.contains("reprap (reprap)") || lower == "reprap" {
            Self::RepRap
        } else {
            Self::Marlin
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Marlin => "marlin",
            Self::RepRap => "reprap",
            Self::Klipper => "klipper",
        }
    }
}

/// One human instruction stored on the job. Exactly one of `layer` or
/// `height_mm` is required, except where a variant only makes sense at a layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CustomInstruction {
    /// Pause the print. `filament_change` selects a filament-change command
    /// instead of a plain pause.
    Pause {
        layer: Option<u32>,
        height_mm: Option<f64>,
        #[serde(default)]
        filament_change: bool,
        message: Option<String>,
    },
    /// Change hotend and/or bed temperature from a layer onwards.
    /// `wait` adds `M109` / `M190` so the printer holds until the new target.
    Temperature {
        layer: u32,
        nozzle_c: Option<f64>,
        bed_c: Option<f64>,
        #[serde(default)]
        wait: bool,
    },
    /// Set the part-cooling fan from a height. `0` turns the fan off.
    Fan { height_mm: f64, percent: f64 },
    /// Short beep (`M300`).
    Beep { layer: u32 },
    /// LCD / console message (`M117`).
    Message { layer: u32, text: String },
    /// Move to a parking position, wait, then come back to the previous XY.
    ParkAndWait {
        layer: u32,
        x: Option<f64>,
        y: Option<f64>,
        message: Option<String>,
    },
}

impl CustomInstruction {
    pub fn summary(&self) -> String {
        match self {
            Self::Pause {
                layer,
                height_mm,
                filament_change,
                message,
            } => {
                let where_ = anchor_label(*layer, *height_mm);
                let why = if *filament_change {
                    "filament change"
                } else {
                    "pause"
                };
                match message {
                    Some(text) if !text.is_empty() => format!("{why} {where_} — {text}"),
                    _ => format!("{why} {where_}"),
                }
            }
            Self::Temperature {
                layer,
                nozzle_c,
                bed_c,
                wait,
            } => {
                let mut bits = Vec::new();
                if let Some(value) = nozzle_c {
                    bits.push(format!("nozzle {value:.0}C"));
                }
                if let Some(value) = bed_c {
                    bits.push(format!("bed {value:.0}C"));
                }
                let hold = if *wait { ", wait" } else { "" };
                format!("temperature from layer {layer} ({}){hold}", bits.join(", "))
            }
            Self::Fan { height_mm, percent } => format!("fan {percent:.0}% from {height_mm:.2} mm"),
            Self::Beep { layer } => format!("beep at layer {layer}"),
            Self::Message { layer, text } => format!("message at layer {layer}: {text}"),
            Self::ParkAndWait { layer, x, y, .. } => format!(
                "park at layer {layer} ({:.1}, {:.1})",
                x.unwrap_or(5.0),
                y.unwrap_or(5.0)
            ),
        }
    }
}

fn anchor_label(layer: Option<u32>, height_mm: Option<f64>) -> String {
    match (layer, height_mm) {
        (Some(layer), _) => format!("at layer {layer}"),
        (None, Some(height)) => format!("at {height:.2} mm"),
        _ => "at an unset position".into(),
    }
}

/// Insert every instruction into `gcode`. The engine file is not modified
/// except through the string this returns.
pub fn apply(
    gcode: &str,
    instructions: &[CustomInstruction],
    flavour: GcodeFlavour,
) -> Result<String, Error> {
    if instructions.is_empty() {
        return Ok(gcode.to_string());
    }
    let document = split_layers(gcode);
    if document.layers.is_empty() {
        return Err(Error::message(
            "the G-code has no ;LAYER: markers, so instructions cannot be placed",
        ));
    }
    let mut document = document;
    let mut planned: Vec<Vec<String>> = vec![Vec::new(); document.layers.len()];
    for instruction in instructions {
        let index = resolve_layer(instruction, &document.layers)?;
        let block = render(instruction, flavour, document.layers[index].position)?;
        planned[index].push(block);
        // CuraEngine keeps writing its own fan and temperature lines after the
        // layer we were asked to change. Rewrite those so the instruction holds.
        stick(&mut document.layers, instruction, index);
    }
    Ok(document.emit(&planned))
}

/// From `start` onward, replace engine fan or temperature commands with the
/// instruction's target. The inserted block itself is not part of the body.
fn stick(layers: &mut [LayerBlock], instruction: &CustomInstruction, start: usize) {
    match instruction {
        CustomInstruction::Fan { percent, .. } => {
            for layer in &mut layers[start..] {
                layer.body = map_lines(&layer.body, |line| rewrite_fan(line, *percent));
            }
        }
        CustomInstruction::Temperature {
            nozzle_c, bed_c, ..
        } => {
            for layer in &mut layers[start..] {
                layer.body = map_lines(&layer.body, |line| {
                    let mut line = line.to_string();
                    if let Some(value) = nozzle_c {
                        if let Some(rewritten) = rewrite_setpoint(&line, &["M104", "M109"], *value)
                        {
                            line = rewritten;
                        }
                    }
                    if let Some(value) = bed_c {
                        if let Some(rewritten) = rewrite_setpoint(&line, &["M140", "M190"], *value)
                        {
                            line = rewritten;
                        }
                    }
                    line
                });
            }
        }
        _ => {}
    }
}

fn map_lines(body: &str, mut edit: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(body.len());
    for line in body.lines() {
        out.push_str(&edit(line));
        out.push('\n');
    }
    out
}

fn rewrite_fan(line: &str, percent: f64) -> String {
    let code = line.split_whitespace().next().unwrap_or("");
    if code != "M106" && code != "M107" {
        return line.to_string();
    }
    if percent <= 0.0 {
        "M107".to_string()
    } else {
        format!("M106 S{}", (percent / 100.0 * 255.0).round() as i32)
    }
}

fn rewrite_setpoint(line: &str, codes: &[&str], value: f64) -> Option<String> {
    let trimmed = line.trim();
    let code = trimmed.split_whitespace().next()?;
    if !codes.contains(&code) {
        return None;
    }
    let mut words: Vec<String> = trimmed.split_whitespace().map(str::to_string).collect();
    let replacement = format!("S{value:.0}");
    if let Some(word) = words
        .iter_mut()
        .skip(1)
        .find(|word| word.starts_with('S') || word.starts_with('s'))
    {
        *word = replacement;
    } else {
        words.insert(1, replacement);
    }
    Some(words.join(" "))
}

struct LayerBlock {
    index: u32,
    /// Z of the first positioning move on this layer, if the file has one.
    z: Option<f64>,
    /// XY at the end of the previous layer, used when parking.
    position: Option<(f64, f64)>,
    header: String,
    body: String,
}

struct Document {
    preamble: String,
    layers: Vec<LayerBlock>,
}

impl Document {
    fn emit(&self, insertions: &[Vec<String>]) -> String {
        let mut out = String::with_capacity(self.preamble.len() + 1024);
        out.push_str(&self.preamble);
        for (layer, blocks) in self.layers.iter().zip(insertions) {
            out.push_str(&layer.header);
            if !layer.header.ends_with('\n') {
                out.push('\n');
            }
            for block in blocks {
                out.push_str(block);
                if !block.ends_with('\n') {
                    out.push('\n');
                }
            }
            out.push_str(&layer.body);
        }
        out
    }
}

fn split_layers(gcode: &str) -> Document {
    let mut preamble = String::new();
    let mut layers = Vec::new();
    let mut current: Option<LayerBlock> = None;
    let mut x = None;
    let mut y = None;
    let mut z = None;
    let mut seen_z_on_layer = false;
    for line in gcode.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(";LAYER:") {
            if let Some(mut layer) = current.take() {
                layer.z = z.filter(|_| seen_z_on_layer).or(layer.z);
                layers.push(layer);
            }
            let index = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(layers.len() as u32);
            current = Some(LayerBlock {
                index,
                z: None,
                position: match (x, y) {
                    (Some(x), Some(y)) => Some((x, y)),
                    _ => None,
                },
                header: line.to_string(),
                body: String::new(),
            });
            seen_z_on_layer = false;
            continue;
        }
        if let Some(layer) = current.as_mut() {
            layer.body.push_str(line);
            layer.body.push('\n');
            if let Some(value) = motion_z(trimmed) {
                z = Some(value);
                if !seen_z_on_layer {
                    layer.z = Some(value);
                    seen_z_on_layer = true;
                }
            }
            if let Some(value) = word_number(trimmed, 'X') {
                x = Some(value);
            }
            if let Some(value) = word_number(trimmed, 'Y') {
                y = Some(value);
            }
        } else {
            preamble.push_str(line);
            preamble.push('\n');
            if let Some(value) = motion_z(trimmed) {
                z = Some(value);
            }
            if let Some(value) = word_number(trimmed, 'X') {
                x = Some(value);
            }
            if let Some(value) = word_number(trimmed, 'Y') {
                y = Some(value);
            }
        }
    }
    if let Some(layer) = current.take() {
        layers.push(layer);
    }
    Document { preamble, layers }
}

fn motion_z(line: &str) -> Option<f64> {
    let code = line.split_whitespace().next().unwrap_or("");
    if matches!(code, "G0" | "G1" | "G00" | "G01") {
        word_number(line, 'Z')
    } else {
        None
    }
}

fn word_number(line: &str, axis: char) -> Option<f64> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b';' {
            break;
        }
        if bytes[index].eq_ignore_ascii_case(&(axis as u8)) {
            let rest = &line[index + 1..];
            let token = rest.split_whitespace().next().unwrap_or(rest);
            let number: String = token
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E'))
                .collect();
            return number.parse().ok();
        }
        index += 1;
    }
    None
}

fn resolve_layer(instruction: &CustomInstruction, layers: &[LayerBlock]) -> Result<usize, Error> {
    let (layer, height) = match instruction {
        CustomInstruction::Pause {
            layer, height_mm, ..
        } => (*layer, *height_mm),
        CustomInstruction::Temperature { layer, .. }
        | CustomInstruction::Beep { layer }
        | CustomInstruction::Message { layer, .. }
        | CustomInstruction::ParkAndWait { layer, .. } => (Some(*layer), None),
        CustomInstruction::Fan { height_mm, .. } => (None, Some(*height_mm)),
    };
    if let Some(layer) = layer {
        return layers
            .iter()
            .position(|item| item.index == layer)
            .ok_or_else(|| {
                Error::message(format!(
                    "layer {layer} is not in this G-code (last layer is {})",
                    layers.last().map(|item| item.index).unwrap_or(0)
                ))
            });
    }
    let Some(height) = height else {
        return Err(Error::message(
            "a pause needs a layer number or a height in millimetres",
        ));
    };
    layers
        .iter()
        .position(|item| item.z.is_some_and(|z| z + 1e-4 >= height))
        .ok_or_else(|| {
            Error::message(format!(
                "no layer reaches {height:.2} mm (top Z is {:.2})",
                layers.iter().filter_map(|item| item.z).fold(0.0, f64::max)
            ))
        })
}

fn render(
    instruction: &CustomInstruction,
    flavour: GcodeFlavour,
    position: Option<(f64, f64)>,
) -> Result<String, Error> {
    let mut lines = vec![format!("; SHALE {}", instruction.summary())];
    match instruction {
        CustomInstruction::Pause {
            filament_change,
            message,
            ..
        } => lines.extend(pause_lines(flavour, *filament_change, message.as_deref())),
        CustomInstruction::Temperature {
            nozzle_c,
            bed_c,
            wait,
            ..
        } => {
            if nozzle_c.is_none() && bed_c.is_none() {
                return Err(Error::message(
                    "a temperature instruction needs a nozzle or bed target",
                ));
            }
            if let Some(value) = nozzle_c {
                lines.push(format!("M104 S{value:.0}"));
                if *wait {
                    lines.push(format!("M109 S{value:.0}"));
                }
            }
            if let Some(value) = bed_c {
                lines.push(format!("M140 S{value:.0}"));
                if *wait {
                    lines.push(format!("M190 S{value:.0}"));
                }
            }
        }
        CustomInstruction::Fan { percent, .. } => {
            if !(0.0..=100.0).contains(percent) {
                return Err(Error::message("fan percent must be between 0 and 100"));
            }
            if *percent <= 0.0 {
                lines.push("M107".into());
            } else {
                let pwm = (*percent / 100.0 * 255.0).round() as i32;
                lines.push(format!("M106 S{pwm}"));
            }
        }
        CustomInstruction::Beep { .. } => lines.push("M300 S440 P200".into()),
        CustomInstruction::Message { text, .. } => lines.push(format!("M117 {}", sanitise(text))),
        CustomInstruction::ParkAndWait { x, y, message, .. } => {
            let (px, py) = (x.unwrap_or(5.0), y.unwrap_or(5.0));
            lines.push(format!("G0 X{px:.3} Y{py:.3} F3000"));
            lines.extend(pause_lines(flavour, false, message.as_deref()));
            if let Some((sx, sy)) = position {
                lines.push(format!("G0 X{sx:.3} Y{sy:.3} F3000"));
            }
        }
    }
    Ok(lines.join("\n") + "\n")
}

fn pause_lines(flavour: GcodeFlavour, filament_change: bool, message: Option<&str>) -> Vec<String> {
    let text = sanitise(message.unwrap_or(if filament_change {
        "Filament change"
    } else {
        "Paused by Shale"
    }));
    match flavour {
        GcodeFlavour::Marlin if filament_change => vec![format!("M600 ; {text}")],
        GcodeFlavour::Marlin => vec![format!("M0 {text}")],
        GcodeFlavour::RepRap => {
            let mut lines = vec!["M226".to_string()];
            lines.push(format!("M117 {text}"));
            if filament_change {
                lines.insert(0, "; RepRap has no standard M600; pausing with M226".into());
            }
            lines
        }
        GcodeFlavour::Klipper if filament_change => vec![format!("M600 ; {text}")],
        GcodeFlavour::Klipper => vec![format!("PAUSE ; {text}")],
    }
}

fn sanitise(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch == ';' || ch == '\n' || ch == '\r' {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        let mut lines = vec!["; header".into(), "G28".into()];
        for layer in 0..50 {
            let z = (layer + 1) as f64 * 0.2;
            lines.push(format!(";LAYER:{layer}"));
            lines.push(format!("G0 Z{z:.1}"));
            lines.push(format!("G1 X10 Y{layer} E1"));
        }
        lines.join("\n") + "\n"
    }

    #[test]
    fn pause_for_filament_change_on_marlin() {
        let gcode = apply(
            &sample(),
            &[CustomInstruction::Pause {
                layer: Some(40),
                height_mm: None,
                filament_change: true,
                message: Some("swap to red".into()),
            }],
            GcodeFlavour::Marlin,
        )
        .unwrap();
        let at = gcode.find(";LAYER:40").unwrap();
        let next = gcode.find(";LAYER:41").unwrap();
        let body = &gcode[at..next];
        assert!(body.contains("M600 ; swap to red"), "{body}");
        assert!(body.find("M600").unwrap() < body.find("G1 X10").unwrap());
    }

    #[test]
    fn temperature_waits_from_the_named_layer() {
        let gcode = apply(
            &sample(),
            &[CustomInstruction::Temperature {
                layer: 5,
                nozzle_c: Some(195.0),
                bed_c: Some(55.0),
                wait: true,
            }],
            GcodeFlavour::Marlin,
        )
        .unwrap();
        let body = &gcode[gcode.find(";LAYER:5").unwrap()..gcode.find(";LAYER:6").unwrap()];
        assert!(body.contains("M104 S195"));
        assert!(body.contains("M109 S195"));
        assert!(body.contains("M140 S55"));
        assert!(body.contains("M190 S55"));
    }

    #[test]
    fn height_pause_lands_on_the_right_layer() {
        let gcode = apply(
            &sample(),
            &[CustomInstruction::Pause {
                layer: None,
                height_mm: Some(8.2),
                filament_change: false,
                message: Some("insert nut".into()),
            }],
            GcodeFlavour::Marlin,
        )
        .unwrap();
        // Z = (index+1)*0.2, so 8.2 is layer index 40.
        let at = gcode.find(";LAYER:40").unwrap();
        let next = gcode.find(";LAYER:41").unwrap();
        let body = &gcode[at..next];
        assert!(body.contains("M0 insert nut"), "{body}");
        assert!(!gcode[gcode.find(";LAYER:39").unwrap()..at].contains("M0"));
    }

    #[test]
    fn fan_beep_message_and_park() {
        let gcode = apply(
            &sample(),
            &[
                CustomInstruction::Fan {
                    height_mm: 1.0,
                    percent: 50.0,
                },
                CustomInstruction::Beep { layer: 2 },
                CustomInstruction::Message {
                    layer: 2,
                    text: "hello;there".into(),
                },
                CustomInstruction::ParkAndWait {
                    layer: 3,
                    x: Some(10.0),
                    y: Some(20.0),
                    message: None,
                },
            ],
            GcodeFlavour::Marlin,
        )
        .unwrap();
        let fan_at = gcode.find(";LAYER:4").unwrap();
        assert!(gcode[fan_at..gcode.find(";LAYER:5").unwrap()].contains("M106 S128"));
        let two = gcode.find(";LAYER:2").unwrap();
        let two_body = &gcode[two..gcode.find(";LAYER:3").unwrap()];
        assert!(two_body.contains("M300 S440 P200"));
        assert!(two_body.contains("M117 hello there"));
        let three = &gcode[gcode.find(";LAYER:3").unwrap()..gcode.find(";LAYER:4").unwrap()];
        assert!(three.contains("G0 X10.000 Y20.000 F3000"));
        assert!(three.contains("M0 Paused by Shale"));
        assert!(three.contains("G0 X10.000 Y2.000 F3000"), "{three}");
    }

    #[test]
    fn reprap_and_klipper_pauses_differ() {
        let pause = CustomInstruction::Pause {
            layer: Some(1),
            height_mm: None,
            filament_change: false,
            message: None,
        };
        let reprap = apply(&sample(), std::slice::from_ref(&pause), GcodeFlavour::RepRap).unwrap();
        assert!(reprap.contains("M226"));
        assert!(!reprap.contains("\nM0 "));
        let klipper = apply(&sample(), &[pause], GcodeFlavour::Klipper).unwrap();
        assert!(klipper.contains("PAUSE"));
    }

    #[test]
    fn flavour_from_cura_names() {
        assert_eq!(
            GcodeFlavour::from_cura("RepRap (Marlin/Sprinter)"),
            GcodeFlavour::Marlin
        );
        assert_eq!(
            GcodeFlavour::from_cura("RepRap (RepRap)"),
            GcodeFlavour::RepRap
        );
        assert_eq!(GcodeFlavour::from_cura("Klipper"), GcodeFlavour::Klipper);
    }

    #[test]
    fn fan_and_temperature_override_later_engine_commands() {
        let gcode = sample().replace("G1 X10", "M106 S255\nM104 S200\nM140 S60\nG1 X10");
        let gcode = apply(
            &gcode,
            &[
                CustomInstruction::Fan {
                    height_mm: 1.0,
                    percent: 50.0,
                },
                CustomInstruction::Temperature {
                    layer: 5,
                    nozzle_c: Some(195.0),
                    bed_c: Some(55.0),
                    wait: false,
                },
            ],
            GcodeFlavour::Marlin,
        )
        .unwrap();
        let layer3 = &gcode[gcode.find(";LAYER:3").unwrap()..gcode.find(";LAYER:4").unwrap()];
        assert!(layer3.contains("M106 S255"), "{layer3}");
        assert!(layer3.contains("M104 S200"));
        let layer4 = &gcode[gcode.find(";LAYER:4").unwrap()..gcode.find(";LAYER:5").unwrap()];
        assert!(layer4.contains("M106 S128"), "{layer4}");
        assert!(!layer4.contains("M106 S255"));
        assert!(layer4.contains("M104 S200"));
        let layer6 = &gcode[gcode.find(";LAYER:6").unwrap()..gcode.find(";LAYER:7").unwrap()];
        assert!(layer6.contains("M106 S128"));
        assert!(layer6.contains("M104 S195"), "{layer6}");
        assert!(layer6.contains("M140 S55"));
        assert!(!layer6.contains("M104 S200"));
        assert!(!layer6.contains("M140 S60"));
    }

    #[test]
    fn missing_layer_is_an_error() {
        let err = apply(
            &sample(),
            &[CustomInstruction::Beep { layer: 90 }],
            GcodeFlavour::Marlin,
        )
        .unwrap_err();
        assert!(err.to_string().contains("layer 90"));
    }
}
