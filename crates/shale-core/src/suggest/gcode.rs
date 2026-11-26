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
    /// Skirt, brim, and raft. Separate so the feature buckets add up to `seconds`.
    pub skirt_seconds: f64,
    /// Extrusion that is not a wall, infill, support, ironing pass, or skirt.
    pub other_seconds: f64,
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
        if let Some(index) = tool_index(head) {
            out.tool_changes = out.tool_changes.saturating_add(1);
            tool = index;
            continue;
        }
        if head.starts_with('T') && head.len() > 1 && head[1..].chars().all(|c| c.is_ascii_digit())
        {
            // T255 and T1000 are Bambu control codes, not extruders.
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
                // `G0 F0 Z…` is a Bambu hop. F0 means "keep the last speed";
                // the printer fills it in. Treating it as 1 mm/min turns a
                // fraction of a millimetre into minutes of travel.
                'F' => {
                    if value > 0.0 {
                        feed = value;
                    }
                }
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
            } else if kind.contains("SKIRT") || kind.contains("BRIM") || kind.contains("RAFT") {
                out.skirt_seconds += seconds;
            } else if kind.contains("WALL") || kind.contains("SKIN") || kind.contains("TOP") {
                out.wall_seconds += seconds;
            } else if kind.contains("FILL") || kind.contains("INFILL") {
                out.infill_seconds += seconds;
            } else {
                out.other_seconds += seconds;
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

/// Real tool indexes. Bambu writes `T255` for a flush and `T1000` to restore
/// the active tool; those are not extruders, and treating `T1000` as tool 1000
/// pads the extruder list with a thousand empty entries.
fn tool_index(head: &str) -> Option<usize> {
    let rest = head.strip_prefix('T')?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let index = rest.parse::<usize>().ok()?;
    if index > 15 {
        None
    } else {
        Some(index)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bambu_z_hop_with_f0_keeps_the_previous_feed() {
        let gcode = "\
G1 F18000 X10 Y0
G0 F0 Z0.4
G0 F0 Z0.8
";
        let facts = facts(gcode);
        // 10 mm at 18 000 mm/min is a third of a second. At 1 mm/min the two
        // hops alone would be nearly a minute.
        assert!(
            facts.travel_seconds < 1.0,
            "travel {} s",
            facts.travel_seconds
        );
        assert!(facts.seconds < 1.0, "total {} s", facts.seconds);
    }

    #[test]
    fn skirt_is_counted_and_the_buckets_sum_to_the_total() {
        let gcode = "\
;TYPE:SKIRT
G1 F1200 X10 Y0 E1
;TYPE:WALL-OUTER
G1 X20 Y0 E2
;TYPE:FILL
G1 X30 Y0 E3
G0 X40 Y0
";
        let facts = facts(gcode);
        let sum = facts.wall_seconds
            + facts.infill_seconds
            + facts.support_seconds
            + facts.travel_seconds
            + facts.ironing_seconds
            + facts.skirt_seconds
            + facts.other_seconds;
        assert!(
            (sum - facts.seconds).abs() < 1e-6,
            "buckets {sum} total {}",
            facts.seconds
        );
        assert!(facts.skirt_seconds > 0.0);
        assert!(facts.wall_seconds > 0.0);
        assert!(facts.infill_seconds > 0.0);
        assert!(facts.travel_seconds > 0.0);
    }

    #[test]
    fn bambu_t1000_and_t255_are_not_extruders() {
        let gcode = "\
T0
G1 F1200 X10 Y0 E1
T1000
G0 X20 Y0
T255
G1 X30 Y0 E2
";
        let facts = facts(gcode);
        assert_eq!(facts.per_tool_mm.len(), 1);
        assert!(facts.per_tool_mm[0] > 0.0);
    }
}
