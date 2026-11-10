//! Turn CuraEngine G-code into polylines the viewport can draw.
//!
//! The CLI backend does not stream layer polygons; that arrives with Arcus.
//! CuraEngine annotates the file with `;LAYER:` and `;TYPE:` comments, which
//! is enough for a layer slider. Moves before the first `;LAYER:` belong to
//! the start script and are left out so homing travels do not fill the view.

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Toolpath {
    pub layers: Vec<LayerPaths>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LayerPaths {
    pub index: i32,
    pub z: f32,
    pub polylines: Vec<Polyline>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Polyline {
    pub kind: String,
    pub points: Vec<[f32; 3]>,
    /// The feedrate these moves asked for, in mm/s. `F` is written in mm/min and
    /// only when it changes, so it is carried forward like the position is; the
    /// preview colours by it, and a polyline breaks where it steps, because a
    /// line drawn at two speeds cannot carry one colour.
    pub feed: f32,
}

/// Fill in header fields the CuraEngine CLI leaves as placeholders.
///
/// `;TIME:6666` is written when the engine finalises the header without a
/// duration. `;Filament used: 0m` is the same kind of gap; the extrusion
/// commands themselves are already in millimetres of filament.
pub fn annotate_header(gcode: &str, print_time_seconds: Option<f64>) -> String {
    let mut text = gcode.to_string();
    let seconds = print_time_seconds.or_else(|| elapsed_seconds(&text));
    if let Some(seconds) = seconds {
        let seconds = seconds.round().max(0.0) as i64;
        text = text.replacen(";TIME:6666", &format!(";TIME:{seconds}"), 1);
    }
    let metres = filament_length_metres(&text);
    if metres > 0.0 {
        text = text.replacen(
            ";Filament used: 0m",
            &format!(";Filament used: {metres:.4}m"),
            1,
        );
    }
    text
}

/// What a finished file's own header says about the job.
///
/// CuraEngine writes `;TIME:` and `;Filament used:` before the first move, and
/// `annotate_header` puts the measured figures back into those lines. Reading
/// them costs a few kilobytes, which is what a plate reopened from disk can
/// afford; measuring the moves again would mean reading the whole file.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeaderFacts {
    pub seconds: Option<f64>,
    pub filament_metres: Option<f64>,
}

pub fn header_facts(gcode: &str) -> HeaderFacts {
    let mut facts = HeaderFacts::default();
    for raw in gcode.lines() {
        let line = raw.trim();
        if !line.starts_with(';') {
            // The header ends at the first command; anything after it is a
            // later marker and not the file's own account of itself.
            break;
        }
        if let Some(value) = line.strip_prefix(";TIME:") {
            // `6666` is CuraEngine's placeholder for "I did not work it out",
            // not an estimate of 111 minutes; `annotate_header` looks for it by
            // that literal, so a reader that reports it as a time would turn a
            // missing estimate into a wrong one.
            facts.seconds = value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| *value > 0.0 && *value != 6666.0);
        } else if let Some(value) = line.strip_prefix(";Filament used:") {
            facts.filament_metres = value
                .trim()
                .trim_end_matches('m')
                .parse::<f64>()
                .ok()
                .filter(|value| *value > 0.0);
        }
    }
    facts
}

fn elapsed_seconds(gcode: &str) -> Option<f64> {
    gcode
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix(";TIME_ELAPSED:")
                .and_then(|value| value.trim().parse::<f64>().ok())
        })
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

/// Filament used while the current `;TYPE:` is support or support interface.
/// Extrusion position is tracked through the whole file, so a later support
/// move is not charged for the walls that came before it.
pub fn support_filament_metres(gcode: &str) -> f64 {
    let mut kind = String::new();
    let mut absolute = true;
    let mut position = 0.0f64;
    let mut total = 0.0f64;
    for raw in gcode.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix(";TYPE:") {
            kind = rest.trim().to_ascii_uppercase();
            continue;
        }
        if line.starts_with(';') {
            continue;
        }
        if line.starts_with("M82") {
            absolute = true;
            continue;
        }
        if line.starts_with("M83") {
            absolute = false;
            continue;
        }
        if line.starts_with("G92") {
            if let Some(value) = word(line, 'E') {
                position = value as f64;
            }
            continue;
        }
        if line.starts_with("G0") || line.starts_with("G1") {
            if let Some(value) = word(line, 'E') {
                let value = value as f64;
                let delta = if absolute {
                    let delta = (value - position).max(0.0);
                    position = value;
                    delta
                } else if value > 0.0 {
                    value
                } else {
                    0.0
                };
                if kind.starts_with("SUPPORT") {
                    total += delta;
                }
            }
        }
    }
    total / 1000.0
}

/// Filament used per tool, in metres, keyed by the tool the G-code selected.
///
/// A plate sliced for two spools carries `T0` for one and `T1` for the other, and
/// the total alone cannot say whether the second spool is needed for a gram or
/// for a hundred — which is the difference between a tool change nobody notices
/// and a print that runs out. The same E accounting as [`filament_length_metres`]:
/// absolute moves add their rise, relative moves add themselves, and `G92 E0` at
/// a tool change only resets the reference.
pub fn filament_by_tool(gcode: &str) -> std::collections::BTreeMap<u8, f64> {
    let mut absolute = true;
    let mut position = 0.0f64;
    let mut tool: u8 = 0;
    let mut totals: std::collections::BTreeMap<u8, f64> = std::collections::BTreeMap::new();
    totals.insert(0, 0.0);
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with(';') {
            continue;
        }
        if line.starts_with("M82") {
            absolute = true;
            continue;
        }
        if line.starts_with("M83") {
            absolute = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix('T') {
            let digits: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(index) = digits.parse::<u8>() {
                tool = index;
                totals.entry(tool).or_insert(0.0);
            }
            continue;
        }
        if line.starts_with("G92") {
            if let Some(value) = word(line, 'E') {
                position = value as f64;
            }
            continue;
        }
        if line.starts_with("G0") || line.starts_with("G1") {
            if let Some(value) = word(line, 'E') {
                let value = value as f64;
                let entry = totals.entry(tool).or_insert(0.0);
                if absolute {
                    if value > position {
                        *entry += value - position;
                    }
                    position = value;
                } else if value > 0.0 {
                    *entry += value;
                }
            }
        }
    }
    for value in totals.values_mut() {
        *value /= 1000.0;
    }
    totals
}

pub fn filament_length_metres(gcode: &str) -> f64 {
    let mut absolute = true;
    let mut position = 0.0f64;
    let mut total = 0.0f64;
    for raw in gcode.lines() {
        let line = raw.trim();
        if line.starts_with(';') {
            continue;
        }
        if line.starts_with("M82") {
            absolute = true;
        } else if line.starts_with("M83") {
            absolute = false;
        } else if line.starts_with("G92") {
            if let Some(value) = word(line, 'E') {
                position = value as f64;
            }
        } else if line.starts_with("G0") || line.starts_with("G1") {
            if let Some(value) = word(line, 'E') {
                let value = value as f64;
                if absolute {
                    if value > position {
                        total += value - position;
                    }
                    position = value;
                } else if value > 0.0 {
                    total += value;
                }
            }
        }
    }
    total / 1000.0
}

pub fn parse_toolpaths(gcode: &str) -> Toolpath {
    let mut state = MotionState::default();
    let mut layers: Vec<LayerPaths> = Vec::new();
    let mut layer_index: Option<i32> = None;
    // The print plane of a layer comes from its first extrusion, not from the
    // last Z seen: Cura lifts Z before the `;LAYER:` comment that ends a layer,
    // and the end G-code moves Z in relative mode after the last layer.
    let mut z_taken = false;

    for raw in gcode.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix(';') {
            let comment = rest.trim();
            if let Some(value) = comment.strip_prefix("LAYER:") {
                state.flush_into(&mut layers);
                let index = value.trim().parse::<i32>().unwrap_or(0);
                layer_index = Some(index);
                z_taken = false;
                layers.push(LayerPaths {
                    index,
                    z: state.z,
                    polylines: Vec::new(),
                });
            } else if let Some(value) = comment.strip_prefix("TYPE:") {
                state.flush_into(&mut layers);
                state.kind = normalise_type(value.trim());
            }
            continue;
        }
        let recording = layer_index.is_some();
        state.apply(line, recording);
        let extruded = state.last_extruded;
        if extruded && !z_taken {
            if let Some(index) = layer_index {
                if let Some(layer) = layers.iter_mut().find(|layer| layer.index == index) {
                    layer.z = state.z;
                    z_taken = true;
                }
            }
        }
    }
    state.flush_into(&mut layers);
    layers.retain(|layer| !layer.polylines.is_empty());
    Toolpath { layers }
}

fn normalise_type(raw: &str) -> String {
    match raw {
        "WALL-OUTER" => "wall-outer".to_string(),
        "WALL-INNER" => "wall-inner".to_string(),
        "SKIN" => "skin".to_string(),
        "FILL" => "infill".to_string(),
        "SUPPORT" => "support".to_string(),
        "SUPPORT-INTERFACE" => "support-interface".to_string(),
        "SKIRT" => "skirt".to_string(),
        "BRIM" => "brim".to_string(),
        "RAFT" => "raft".to_string(),
        "PRIME-TOWER" => "prime-tower".to_string(),
        other => {
            let lower = other.to_ascii_lowercase();
            if lower.is_empty() {
                "extrusion".to_string()
            } else {
                lower
            }
        }
    }
}

struct MotionState {
    absolute_xyz: bool,
    absolute_e: bool,
    x: f32,
    y: f32,
    z: f32,
    e: f32,
    unit: f32,
    feed: f32,
    kind: String,
    current: Option<Polyline>,
    pending: Vec<Polyline>,
    last_extruded: bool,
}

impl Default for MotionState {
    fn default() -> Self {
        Self {
            absolute_xyz: true,
            absolute_e: true,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            e: 0.0,
            unit: 1.0,
            feed: 0.0,
            kind: "extrusion".to_string(),
            current: None,
            pending: Vec::new(),
            last_extruded: false,
        }
    }
}

impl MotionState {
    fn apply(&mut self, line: &str, recording: bool) {
        let code = line.split_whitespace().next().unwrap_or("");
        self.last_extruded = false;
        match code {
            "G90" => self.absolute_xyz = true,
            "G91" => self.absolute_xyz = false,
            "M82" => self.absolute_e = true,
            "M83" => self.absolute_e = false,
            "G20" => self.unit = 25.4,
            "G21" => self.unit = 1.0,
            "G92" => {
                if let Some(value) = word(line, 'X') {
                    self.x = value * self.unit;
                }
                if let Some(value) = word(line, 'Y') {
                    self.y = value * self.unit;
                }
                if let Some(value) = word(line, 'Z') {
                    self.z = value * self.unit;
                }
                if let Some(value) = word(line, 'E') {
                    self.e = value;
                }
            }
            "G0" | "G1" | "G00" | "G01" => {
                let previous = [self.x, self.y, self.z];
                let previous_e = self.e;
                if let Some(value) = word(line, 'X') {
                    let value = value * self.unit;
                    self.x = if self.absolute_xyz {
                        value
                    } else {
                        self.x + value
                    };
                }
                if let Some(value) = word(line, 'Y') {
                    let value = value * self.unit;
                    self.y = if self.absolute_xyz {
                        value
                    } else {
                        self.y + value
                    };
                }
                if let Some(value) = word(line, 'Z') {
                    let value = value * self.unit;
                    self.z = if self.absolute_xyz {
                        value
                    } else {
                        self.z + value
                    };
                }
                if let Some(value) = word(line, 'F') {
                    self.feed = (value / 60.0).max(0.0);
                }
                let mut extruding = false;
                if let Some(value) = word(line, 'E') {
                    if self.absolute_e {
                        extruding = value > previous_e + 1e-6;
                        self.e = value;
                    } else {
                        extruding = value > 1e-6;
                        self.e += value;
                    }
                }
                let next = [self.x, self.y, self.z];
                if !recording || next == previous {
                    return;
                }
                let kind = if extruding {
                    self.kind.clone()
                } else {
                    "travel".to_string()
                };
                self.push(previous, next, &kind, self.feed);
                self.last_extruded = extruding;
            }
            _ => {}
        }
    }

    fn push(&mut self, from: [f32; 3], to: [f32; 3], kind: &str, feed: f32) {
        let continues = self.current.as_ref().is_some_and(|polyline| {
            polyline.kind == kind
                && (polyline.feed - feed).abs() < 0.5
                && polyline.points.last().copied() == Some(from)
        });
        if continues {
            if let Some(polyline) = self.current.as_mut() {
                polyline.points.push(to);
            }
            return;
        }
        self.close_current();
        self.current = Some(Polyline {
            kind: kind.to_string(),
            points: vec![from, to],
            feed,
        });
    }

    fn close_current(&mut self) {
        if let Some(polyline) = self.current.take() {
            if polyline.points.len() >= 2 {
                self.pending.push(polyline);
            }
        }
    }

    fn flush_into(&mut self, layers: &mut [LayerPaths]) {
        self.close_current();
        if self.pending.is_empty() {
            return;
        }
        if let Some(layer) = layers.last_mut() {
            layer.polylines.append(&mut self.pending);
        } else {
            self.pending.clear();
        }
    }
}

fn word(line: &str, axis: char) -> Option<f32> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b';' {
            break;
        }
        if bytes[index].eq_ignore_ascii_case(&(axis as u8)) {
            let rest = &line[index + 1..];
            let token = rest
                .split_whitespace()
                .next()
                .unwrap_or(rest)
                .trim_end_matches(';');
            let number: String = token
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E'))
                .collect();
            if let Ok(value) = number.parse::<f32>() {
                return Some(value);
            }
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_and_extrusion_kinds() {
        let gcode = "\
G28
;LAYER:0
;TYPE:WALL-OUTER
G0 X0 Y0 Z0.2
G1 X10 Y0 E1.0
;TYPE:FILL
G1 X10 Y10 E2.0
G0 X0 Y0
;LAYER:1
;TYPE:WALL-OUTER
G1 Z0.4
G1 X5 Y5 E3.0
";
        let toolpath = parse_toolpaths(gcode);
        assert_eq!(toolpath.layers.len(), 2);
        let first = &toolpath.layers[0];
        assert_eq!(first.index, 0);
        let kinds: Vec<_> = first
            .polylines
            .iter()
            .map(|path| path.kind.as_str())
            .collect();
        assert!(kinds.contains(&"wall-outer"), "{kinds:?}");
        assert!(kinds.contains(&"infill"), "{kinds:?}");
        assert!(kinds.contains(&"travel"), "{kinds:?}");
        assert_eq!(toolpath.layers[1].index, 1);
        assert!(toolpath.layers[1]
            .polylines
            .iter()
            .any(|path| path.kind == "wall-outer"));
    }

    #[test]
    fn layer_z_comes_from_the_first_extrusion_not_the_end_gcode() {
        // Shape of a Cura 5.13 file: the lift to the next layer is written at
        // the end of the previous layer's block, and the end G-code moves Z in
        // relative mode after the last `;LAYER:`.
        let gcode = "\
;LAYER:0
;TYPE:WALL-OUTER
G0 X0 Y0 Z0.2
G1 X10 Y0 E1.0
G1 X10 Y10 E2.0
G0 F300 Z0.4
;LAYER:1
;TYPE:WALL-OUTER
G1 X0 Y0 E3.0
G1 X10 Y0 E4.0
;TIME_ELAPSED:10.0
G1 F2700 E-1.0
G91
G1 E-2 F2700
G1 E-2 Z0.2 F2400
G1 Z10
G90
M84
";
        let toolpath = parse_toolpaths(gcode);
        assert_eq!(toolpath.layers.len(), 2);
        assert_eq!(toolpath.layers[0].z, 0.2);
        assert_eq!(toolpath.layers[1].z, 0.4);
    }
}

#[cfg(test)]
mod filament_tests {
    use super::*;

    #[test]
    fn a_second_spool_is_counted_on_its_own() {
        // 1,000 mm on T0, then G92 E0, then 250 mm on T1: the absolute
        // reference restarts, and the two spools must not be added together.
        let gcode = "M82\nG1 X1 E1000\nT1\nG92 E0\nG1 X2 E250\n";
        let by_tool = filament_by_tool(gcode);
        assert_eq!(by_tool.get(&0).copied(), Some(1.0));
        assert_eq!(by_tool.get(&1).copied(), Some(0.25));
    }

    #[test]
    fn relative_extrusion_is_per_tool_too() {
        let gcode = "M83\nT0\nG1 E10\nT1\nG1 E5\nG1 E5\n";
        let by_tool = filament_by_tool(gcode);
        assert_eq!(by_tool.get(&0).copied(), Some(0.01));
        assert_eq!(by_tool.get(&1).copied(), Some(0.01));
    }

    #[test]
    fn a_single_tool_file_is_one_entry() {
        let by_tool = filament_by_tool("M83\nG1 E40\n");
        assert_eq!(by_tool.len(), 1);
        assert_eq!(by_tool.get(&0).copied(), Some(0.04));
    }
}
