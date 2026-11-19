//! Swept toolhead envelope check against already-printed material.

use crate::config::{CollisionAdjustment, NonplanarReport};
use crate::deform::DeformField;
use crate::envelope::HeadEnvelope;

/// Coarse printed-height field built while sweeping moves in layer order.
#[derive(Clone, Debug, Default)]
pub struct PrintedHeightMap {
    pub origin: [f64; 2],
    pub step: f64,
    pub width: usize,
    pub height: usize,
    pub z: Vec<f64>,
}

impl PrintedHeightMap {
    pub fn new(origin: [f64; 2], step: f64, width: usize, height: usize) -> Self {
        Self {
            origin,
            step,
            width,
            height,
            z: vec![0.0; width * height],
        }
    }

    fn index(&self, x: f64, y: f64) -> Option<usize> {
        let ix = ((x - self.origin[0]) / self.step).round() as isize;
        let iy = ((y - self.origin[1]) / self.step).round() as isize;
        if ix < 0 || iy < 0 {
            return None;
        }
        let ix = ix as usize;
        let iy = iy as usize;
        if ix >= self.width || iy >= self.height {
            return None;
        }
        Some(iy * self.width + ix)
    }

    pub fn raise(&mut self, x: f64, y: f64, z: f64) {
        if let Some(i) = self.index(x, y) {
            self.z[i] = self.z[i].max(z);
        }
    }

    pub fn sample_max_in_radius(&self, x: f64, y: f64, radius: f64) -> f64 {
        let ri = (radius / self.step).ceil() as isize;
        let fx = ((x - self.origin[0]) / self.step).round() as isize;
        let fy = ((y - self.origin[1]) / self.step).round() as isize;
        let mut max_z = 0.0f64;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                let ix = fx + dx;
                let iy = fy + dy;
                if ix < 0 || iy < 0 {
                    continue;
                }
                let ix = ix as usize;
                let iy = iy as usize;
                if ix >= self.width || iy >= self.height {
                    continue;
                }
                let dist = ((dx as f64 * self.step).powi(2) + (dy as f64 * self.step).powi(2)).sqrt();
                if dist <= radius + self.step {
                    max_z = max_z.max(self.z[iy * self.width + ix]);
                }
            }
        }
        max_z
    }
}

/// Returns `true` if the sock / body above the nozzle tip would strike **earlier**
/// printed material. The nozzle tip may contact the layer currently being deposited.
pub fn move_collides(
    envelope: &HeadEnvelope,
    printed: &PrintedHeightMap,
    x: f64,
    y: f64,
    z_nozzle: f64,
    max_angle_deg: f64,
    extruding: bool,
) -> bool {
    if extruding {
        return false;
    }
    let body_r = envelope
        .sock_outer_radius_mm
        .max(envelope.body_radius_mm);
    let material_z = printed.sample_max_in_radius(x, y, body_r);
    if material_z <= 0.0 {
        return false;
    }
    // Material at or below the tip is the layer being laid — not an envelope strike.
    if material_z <= z_nozzle + 0.05 {
        return false;
    }
    let sock_top_z = z_nozzle + envelope.sock_top_z_mm;
    if material_z <= sock_top_z {
        return false;
    }
    let _ = max_angle_deg;
    let max_slope = max_angle_deg.to_radians().tan().max(1e-6);
    let dz = material_z - sock_top_z;
    dz < body_r / max_slope
}

/// Reduce `field.island_strength` where moves collide; update report counters.
pub fn flatten_on_collision(
    field: &mut DeformField,
    gcode: &str,
    envelope: &HeadEnvelope,
    max_angle_deg: f64,
) -> NonplanarReport {
    let mut report = NonplanarReport {
        max_angle_deg,
        ..Default::default()
    };
    report.top_islands = crate::height_field::count_top_islands(&field.grid);

    report.envelope_id = envelope.printer_id.clone();
    if !envelope.calibrated {
        report.notes.push(
            "collision flattening skipped (uncalibrated head envelope)".into(),
        );
        return report;
    }
    if envelope.calibration_tier.as_deref() == Some("conservative") {
        report.notes.push(
            "collision flattening uses conservative envelope; measure sock/duct per JSON".into(),
        );
    }

    let step = field.grid.step;
    let origin = field.grid.origin;
    let width = field.grid.width;
    let height = field.grid.height;
    let mut printed = PrintedHeightMap::new(origin, step, width, height);

    let mut position = [0.0; 3];
    let mut collisions = 0usize;
    for raw in gcode.lines() {
        let line = raw.trim();
        if !(line.starts_with('G')
            && (line.contains('X') || line.contains('Y') || line.contains('Z')))
        {
            continue;
        }
        if let Some(x) = word(line, 'X') {
            position[0] = x;
        }
        if let Some(y) = word(line, 'Y') {
            position[1] = y;
        }
        if let Some(z) = word(line, 'Z') {
            position[2] = z;
        }
        let extrude = line.starts_with("G1") && line.contains('E');
        let e_pos = word(line, 'E').unwrap_or(0.0);
        let extruding = extrude && e_pos > 0.0;
        if move_collides(
            envelope,
            &printed,
            position[0],
            position[1],
            position[2],
            max_angle_deg,
            extruding,
        ) {
            collisions += 1;
            report.collision_adjustments.push(CollisionAdjustment {
                x: position[0],
                y: position[1],
                z: position[2],
                action: "dampen_island".into(),
            });
            dampen_island(field, position[0], position[1], 0.35);
        }
        if extruding {
            printed.raise(position[0], position[1], position[2]);
        }
    }

    if collisions > 0 {
        report.islands_flattened = collisions.min(report.top_islands);
        report.notes.push(format!("{collisions} travel moves failed envelope clearance"));
    }
    if field.island_strength.iter().all(|&s| s < 0.05) {
        report.fallback_to_planar = true;
        report.notes.push("all islands flattened; output approaches planar".into());
    }
    report
}

fn dampen_island(field: &mut DeformField, x: f64, y: f64, factor: f64) {
    let fx = (x - field.grid.origin[0]) / field.grid.step;
    let fy = (y - field.grid.origin[1]) / field.grid.step;
    let ix = fx.round().clamp(0.0, field.grid.width as f64 - 1.0) as usize;
    let iy = fy.round().clamp(0.0, field.grid.height as f64 - 1.0) as usize;
    let i = iy * field.grid.width + ix;
    if let Some(s) = field.island_strength.get_mut(i) {
        *s *= factor;
    }
}

fn word(line: &str, letter: char) -> Option<f64> {
    let idx = line.find(letter)?;
    let rest = &line[idx + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deform::build_deform_field;
    use crate::envelope::bambu_a1_envelope;
    use crate::height_field::build_height_field;
    use crate::test_meshes::box_mesh;

    #[test]
    fn collision_detects_tall_pillar_beside_low_travel() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(10.0, 10.0, 25.0);
        assert!(move_collides(
            envelope,
            &printed,
            12.0,
            10.0,
            5.0,
            10.0,
            false
        ));
    }

    #[test]
    fn deposition_extrusion_never_collides() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 32, 32);
        printed.raise(10.0, 10.0, 5.0);
        assert!(!move_collides(
            envelope,
            &printed,
            10.0,
            10.0,
            5.0,
            10.0,
            true
        ));
    }

    #[test]
    fn gentle_dome_travel_over_own_layer_is_clear() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(10.0, 10.0, 23.8);
        assert!(!move_collides(
            envelope,
            &printed,
            10.0,
            10.0,
            24.0,
            10.0,
            false
        ));
    }

    #[test]
    fn flatten_runs_with_conservative_a1_envelope() {
        let mesh = box_mesh(20.0, 20.0, 10.0);
        let grid = build_height_field(&mesh, 1.0, 30.0, 2.0);
        let mut field = build_deform_field(grid, 2.0, 0.4, 0.2, 30.0);
        let gcode = "G0 X0 Y0 Z0.2\nG1 X10 Y10 Z0.3 E1\n";
        let report = flatten_on_collision(&mut field, gcode, bambu_a1_envelope(), 10.0);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("conservative") || n.contains("post-pass")),
            "{:?}",
            report.notes
        );
        assert_eq!(report.envelope_id, "bambu_lab_a1");
    }

    #[test]
    fn concat_pillar_gcode_produces_collisions_when_present() {
        let path = std::path::Path::new("/tmp/shale-pillar-concat.gcode");
        if !path.is_file() {
            return;
        }
        let gcode = std::fs::read_to_string(path).expect("concat gcode");
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([99.8, 104.8], 0.35, 163, 134);
        let mut position = [0.0; 3];
        let mut collisions = 0usize;
        let mut extrusions = 0usize;
        let mut max_printed = 0.0f64;
        for raw in gcode.lines() {
            let line = raw.trim();
            if !(line.starts_with('G')
                && (line.contains('X') || line.contains('Y') || line.contains('Z')))
            {
                continue;
            }
            if let Some(x) = word(line, 'X') {
                position[0] = x;
            }
            if let Some(y) = word(line, 'Y') {
                position[1] = y;
            }
            if let Some(z) = word(line, 'Z') {
                position[2] = z;
            }
            let extrude = line.starts_with("G1") && line.contains('E');
            let e_pos = word(line, 'E').unwrap_or(0.0);
            let extruding = extrude && e_pos > 0.0;
            if move_collides(
                envelope,
                &printed,
                position[0],
                position[1],
                position[2],
                10.0,
                extruding,
            ) {
                collisions += 1;
            }
            if extruding {
                printed.raise(position[0], position[1], position[2]);
                extrusions += 1;
                max_printed = max_printed.max(position[2]);
            }
        }
        assert!(
            extrusions > 1000 && max_printed > 40.0,
            "raises={extrusions} max_z={max_printed:.2}"
        );
        assert!(
            collisions > 100,
            "expected pillar→dome travels to strike tall pillar, got {collisions} (max_z={max_printed:.2})"
        );
    }

    #[test]
    fn pillar_scene_produces_collision_on_travel() {
        let envelope = bambu_a1_envelope();
        let mut printed = PrintedHeightMap::new([0.0, 0.0], 1.0, 64, 64);
        printed.raise(20.0, 0.0, 28.0);
        assert!(move_collides(
            envelope,
            &printed,
            18.0,
            0.0,
            2.5,
            10.0,
            false
        ));
    }
}
