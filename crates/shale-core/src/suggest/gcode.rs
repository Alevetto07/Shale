//! Times taken from a finished G-code file.
//!
//! The walk is one pass over the text. It does not build a toolpath.

#[derive(Clone, Debug, Default)]
pub struct GcodeFacts {
    pub seconds: f64,
    pub support_seconds: f64,
    pub travel_seconds: f64,
    pub ironing_seconds: f64,
    pub wall_seconds: f64,
    pub infill_seconds: f64,
    pub tool_changes: u32,
    pub extrude_mm: f64,
    pub max_speed_mm_s: f64,
    pub average_speed_mm_s: f64,
    pub layer_count: u32,
    /// Filament length per extruder, in millimetres. Index 0 is T0.
    pub per_tool_mm: Vec<f64>,
    /// Filament spent on a prime, wipe, or purge feature.
    pub purge_mm: f64,
    /// Seconds spent on each `;LAYER:` block, in order.
    pub layer_seconds: Vec<f64>,
}

pub fn facts(gcode: &str) -> GcodeFacts {
    let mut out = GcodeFacts::default();
    let mut absolute_xyz = true;
    let mut absolute_e = true;
    let mut pos = [0.0; 3];
    let mut e = 0.0;
    let mut feed = 1500.0;
    let mut kind = String::new();
    let mut speed_sum = 0.0;
    let mut speed_n = 0.0;
    let mut tool = 0usize;
    let mut layer_open = false;
    for raw in gcode.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        let comment = raw
            .split_once(';')
            .map(|(_, rest)| rest.trim())
            .unwrap_or("");
        if let Some(feature) = comment.strip_prefix("TYPE:") {
            kind = feature.trim().to_ascii_uppercase();
        }
        if comment.starts_with("LAYER:") {
            out.layer_count = out.layer_count.saturating_add(1);
            out.layer_seconds.push(0.0);
            layer_open = true;
        }
        if line.is_empty() {
            continue;
        }
        let head = line.split_whitespace().next().unwrap_or("");
        if head == "G90" {
            absolute_xyz = true;
            continue;
        }
        if head == "G91" {
            absolute_xyz = false;
            continue;
        }
        if head == "M82" {
            absolute_e = true;
            continue;
        }
        if head == "M83" {
            absolute_e = false;
            continue;
        }
        if head.starts_with('T') && head[1..].chars().all(|c| c.is_ascii_digit()) && head.len() > 1
        {
            out.tool_changes = out.tool_changes.saturating_add(1);
            if let Ok(index) = head[1..].parse::<usize>() {
                tool = index;
            }
            continue;
        }
        if head == "G92" {
            for word in line.split_whitespace().skip(1) {
                apply_word(word, true, true, &mut pos, &mut e);
            }
            continue;
        }
        if head != "G0" && head != "G1" && head != "G00" && head != "G01" {
            continue;
        }
        let mut next = pos;
        let mut next_e = e;
        let mut moved_e = false;
        for word in line.split_whitespace().skip(1) {
            let Some(axis) = word.chars().next() else {
                continue;
            };
            let Ok(value) = word[1..].parse::<f64>() else {
                continue;
            };
            match axis {
                'F' => feed = value.max(1.0),
                'X' => next[0] = if absolute_xyz { value } else { pos[0] + value },
                'Y' => next[1] = if absolute_xyz { value } else { pos[1] + value },
                'Z' => next[2] = if absolute_xyz { value } else { pos[2] + value },
                'E' => {
                    next_e = if absolute_e { value } else { e + value };
                    moved_e = true;
                }
                _ => {}
            }
        }
        let delta = [next[0] - pos[0], next[1] - pos[1], next[2] - pos[2]];
        let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
        let extruding = moved_e && next_e > e + 1e-6;
        if distance > 1e-6 {
            let seconds = distance / (feed / 60.0);
            out.seconds += seconds;
            let speed = feed / 60.0;
            out.max_speed_mm_s = out.max_speed_mm_s.max(speed);
            speed_sum += speed;
            speed_n += 1.0;
            if layer_open {
                if let Some(bucket) = out.layer_seconds.last_mut() {
                    *bucket += seconds;
                }
            }
            if !extruding {
                out.travel_seconds += seconds;
            } else if kind.contains("SUPPORT") {
                out.support_seconds += seconds;
            } else if kind.contains("IRON") {
                out.ironing_seconds += seconds;
            } else if kind.contains("WALL") || kind.contains("SKIN") || kind.contains("TOP") {
                out.wall_seconds += seconds;
            } else if kind.contains("FILL") || kind.contains("INFILL") {
                out.infill_seconds += seconds;
            }
        }
        if extruding {
            let length = next_e - e;
            out.extrude_mm += length;
            if out.per_tool_mm.len() <= tool {
                out.per_tool_mm.resize(tool + 1, 0.0);
            }
            out.per_tool_mm[tool] += length;
            let purge = kind.contains("PRIME") || kind.contains("WIPE") || kind.contains("PURGE");
            if purge {
                out.purge_mm += length;
            }
        }
        pos = next;
        if moved_e {
            e = next_e;
        }
    }
    if speed_n > 0.0 {
        out.average_speed_mm_s = speed_sum / speed_n;
    }
    out
}

fn apply_word(word: &str, absolute_xyz: bool, absolute_e: bool, pos: &mut [f64; 3], e: &mut f64) {
    let Some(axis) = word.chars().next() else {
        return;
    };
    let Ok(value) = word[1..].parse::<f64>() else {
        return;
    };
    match axis {
        'X' => pos[0] = if absolute_xyz { value } else { pos[0] + value },
        'Y' => pos[1] = if absolute_xyz { value } else { pos[1] + value },
        'Z' => pos[2] = if absolute_xyz { value } else { pos[2] + value },
        'E' => *e = if absolute_e { value } else { *e + value },
        _ => {}
    }
}
