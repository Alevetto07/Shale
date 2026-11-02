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
        if let Some(index) = layer_index {
            if let Some(layer) = layers.iter_mut().find(|layer| layer.index == index) {
                if state.z > 0.0 {
                    layer.z = state.z;
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
    kind: String,
    current: Option<Polyline>,
    pending: Vec<Polyline>,
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
            kind: "extrusion".to_string(),
            current: None,
            pending: Vec::new(),
        }
    }
}

impl MotionState {
    fn apply(&mut self, line: &str, recording: bool) {
        let code = line.split_whitespace().next().unwrap_or("");
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
                self.push(previous, next, &kind);
            }
            _ => {}
        }
    }

    fn push(&mut self, from: [f32; 3], to: [f32; 3], kind: &str) {
        let continues = self.current.as_ref().is_some_and(|polyline| {
            polyline.kind == kind && polyline.points.last().copied() == Some(from)
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
}
