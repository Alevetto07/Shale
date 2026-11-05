//! Calibration prints.
//!
//! Bambu Studio's Calibration tab prints a part once per setting value and asks
//! you to pick the best band. Shale starts with the temperature tower, because
//! it needs no new engine feature: the geometry is a stack of blocks, and the
//! step between bands is a single `M104` written into the slice.
//!
//! The tower alternates a narrow block with a wider one, so every band meets
//! the same overhang on both sides — the difference between bands is then the
//! temperature and nothing else.

use crate::mesh::TriangleMesh;

/// How tall one band of the tower is, and so also the layer span the hotend
/// gets to reach the next temperature.
pub const BAND_HEIGHT_MM: f64 = 10.0;

/// One band of a calibration print.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    /// 0 for the first band.
    pub index: usize,
    /// Where the band starts, in mm above the plate.
    pub z_mm: f64,
    /// The value this band is printed at. For the temperature tower, °C.
    pub temperature: f64,
}

/// A calibration tower and the plan it was built from.
pub struct Tower {
    pub mesh: TriangleMesh,
    pub bands: Vec<Band>,
    pub band_height_mm: f64,
}

/// Where a band swallowed a line.
#[derive(Clone, Debug, PartialEq)]
pub struct Injection {
    pub band: usize,
    pub layer: usize,
    pub z_mm: f64,
    pub temperature: f64,
}

/// A tower of `bands` blocks, each `band_height` tall, alternating 20 mm and
/// 26 mm square so every band overhangs the one below by 3 mm a side.
pub fn temperature_tower(
    center: [f64; 2],
    bands: usize,
    band_height: f64,
    start_c: f64,
    step_c: f64,
) -> Tower {
    let bands = bands.max(1);
    let band_height = if band_height > 0.0 {
        band_height
    } else {
        BAND_HEIGHT_MM
    };
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    for i in 0..bands {
        let width = if i % 2 == 0 { 20.0 } else { 26.0 };
        let half = width / 2.0;
        let z0 = i as f64 * band_height;
        append_box(
            &mut mesh,
            [center[0] - half, center[1] - half, z0],
            [center[0] + half, center[1] + half, z0 + band_height],
        );
    }
    Tower {
        mesh,
        bands: (0..bands)
            .map(|i| Band {
                index: i,
                z_mm: i as f64 * band_height,
                temperature: start_c - step_c * i as f64,
            })
            .collect(),
        band_height_mm: band_height,
    }
}

/// The whole point of a temperature tower is that the print gets cooler as it
/// rises. A negative step would instead heat it up, which is a different test.
pub fn cooling_step(start_c: f64, step_c: f64, bands: usize) -> bool {
    step_c > 0.0 && start_c - step_c * bands.saturating_sub(1) as f64 > 0.0
}

/// Rewrite a slice so each band is printed at its own temperature.
///
/// The hotend is stepped with `M104`, which sets the target without waiting, so
/// the first layer of a band is printed while the nozzle is still catching up.
/// Waiting (`M109`) would stop the print dead at every band, and waiting is not
/// what a temperature tower wants: it wants the band at temperature, and the
/// band is ten millimetres tall, which is time enough on an A1.
pub fn step_temperatures(gcode: &str, bands: &[Band]) -> (String, Vec<Injection>) {
    if bands.is_empty() {
        return (gcode.to_string(), Vec::new());
    }
    // First pass: which height is each layer printed at? CuraEngine writes the
    // layer header, then the first move of the layer carries the Z.
    let mut layer_z: Vec<f64> = Vec::new();
    for line in gcode.lines() {
        if line.starts_with(";LAYER:") {
            layer_z.push(f64::NAN);
            continue;
        }
        if line.starts_with("G0") || line.starts_with("G1") {
            if let Some(z) = parse_z(line) {
                if let Some(slot) = layer_z.last_mut() {
                    if slot.is_nan() {
                        *slot = z;
                    }
                }
            }
        }
    }
    // A layer with no Z of its own is at the last height seen before it.
    let mut last_z = 0.0;
    for z in layer_z.iter_mut() {
        if z.is_nan() {
            *z = last_z;
        } else {
            last_z = *z;
        }
    }

    let band_of = |z: f64| -> Option<usize> {
        for band in bands.iter().rev() {
            if z + 1e-6 >= band.z_mm {
                return Some(band.index);
            }
        }
        None
    };
    let mut plan: Vec<Option<usize>> = Vec::with_capacity(layer_z.len());
    let mut previous: Option<usize> = None;
    for z in &layer_z {
        let band = band_of(*z).or(previous);
        plan.push(band);
        if band.is_some() {
            previous = band;
        }
    }

    // Second pass: write the file back with a step at the first layer of every
    // band, and a banner up top saying what was asked for.
    let mut out = String::with_capacity(gcode.len() + 64 * bands.len() + 256);
    let mut banner = String::new();
    banner.push_str(";SHALE-CALIBRATION temperature-tower\n");
    for band in bands {
        banner.push_str(&format!(
            ";SHALE-CALIBRATION band {} from z={:.2} at {:.0}C\n",
            band.index, band.z_mm, band.temperature
        ));
    }
    let mut injected: Vec<Injection> = Vec::new();
    let mut wrote_banner = false;
    let mut layer = 0usize;
    for line in gcode.split_inclusive('\n') {
        if line.starts_with(";LAYER:") {
            let index = layer;
            if let Some(Some(band)) = plan.get(index) {
                let first_of_band = index == 0
                    || plan.get(index.wrapping_sub(1)).copied().flatten() != Some(*band);
                if first_of_band {
                    if let Some(step) = bands.iter().find(|b| b.index == *band) {
                        out.push_str(&format!(
                            ";CALIBRATION band {} {:.0}C\nM104 S{:.0} ; band {} at {:.0}C\n",
                            step.index, step.temperature, step.temperature, step.index,
                            step.temperature
                        ));
                        injected.push(Injection {
                            band: step.index,
                            layer: index,
                            z_mm: layer_z.get(index).copied().unwrap_or(0.0),
                            temperature: step.temperature,
                        });
                    }
                }
            }
            layer += 1;
        }
        if !wrote_banner && line.starts_with(";FLAVOR") {
            out.push_str(&banner);
            wrote_banner = true;
        }
        out.push_str(line);
    }
    if !wrote_banner {
        let mut with_banner = banner;
        with_banner.push_str(&out);
        out = with_banner;
    }
    (out, injected)
}

fn parse_z(line: &str) -> Option<f64> {
    for token in line.split_whitespace() {
        if let Some(value) = token.strip_prefix('Z') {
            if let Ok(z) = value.parse::<f64>() {
                return Some(z);
            }
        }
    }
    None
}

fn append_box(mesh: &mut TriangleMesh, min: [f64; 3], max: [f64; 3]) {
    let base = mesh.vertices.len() as u32;
    for corner in 0..8 {
        let x = if corner & 1 == 0 { min[0] } else { max[0] };
        let y = if corner & 2 == 0 { min[1] } else { max[1] };
        let z = if corner & 4 == 0 { min[2] } else { max[2] };
        mesh.vertices.push([x as f32, y as f32, z as f32]);
    }
    // Counter-clockwise seen from outside, so the winding agrees with the rest
    // of the meshes and a signed volume comes out positive.
    let faces: [[u32; 3]; 12] = [
        [0, 2, 1],
        [1, 2, 3], // bottom
        [4, 5, 6],
        [5, 7, 6], // top
        [0, 1, 4],
        [1, 5, 4], // front
        [2, 6, 3],
        [3, 6, 7], // back
        [0, 4, 2],
        [2, 4, 6], // left
        [1, 3, 5],
        [3, 7, 5], // right
    ];
    for face in faces {
        mesh.triangles
            .push([base + face[0], base + face[1], base + face[2]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::signed_volume;

    #[test]
    fn the_tower_is_a_stack_of_closed_blocks() {
        let tower = temperature_tower([128.0, 128.0], 5, 10.0, 230.0, 5.0);
        assert_eq!(tower.bands.len(), 5);
        assert_eq!(tower.mesh.triangles.len(), 5 * 12);
        let volume = signed_volume(&tower.mesh);
        // Three narrow blocks (20 mm) and two wide ones (26 mm), ten tall.
        let expected = (3.0 * 20.0 * 20.0 + 2.0 * 26.0 * 26.0) * 10.0;
        assert!(
            (volume - expected).abs() < 1.0,
            "volume {volume} against {expected}"
        );
        let top = tower
            .mesh
            .vertices
            .iter()
            .map(|v| v[2] as f64)
            .fold(f64::MIN, f64::max);
        assert!((top - 50.0).abs() < 1e-6, "tower is {} mm tall", top);
        // Every edge twice: the tower slices without repair.
        let mut edges = std::collections::HashMap::new();
        for [a, b, c] in &tower.mesh.triangles {
            for (u, v) in [(a, b), (b, c), (c, a)] {
                let key = if u < v { (*u, *v) } else { (*v, *u) };
                *edges.entry(key).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|count| *count == 2));
    }

    #[test]
    fn the_bands_cool_by_the_step_asked_for() {
        let tower = temperature_tower([128.0, 128.0], 5, 10.0, 230.0, 5.0);
        let temps: Vec<f64> = tower.bands.iter().map(|b| b.temperature).collect();
        assert_eq!(temps, vec![230.0, 225.0, 220.0, 215.0, 210.0]);
        let zs: Vec<f64> = tower.bands.iter().map(|b| b.z_mm).collect();
        assert_eq!(zs, vec![0.0, 10.0, 20.0, 30.0, 40.0]);
        assert!(cooling_step(230.0, 5.0, 5));
        assert!(!cooling_step(230.0, -5.0, 5));
        assert!(!cooling_step(20.0, 5.0, 5));
    }

    #[test]
    fn the_hotend_steps_once_per_band() {
        // Forty layers of 0.2 mm: 8 mm, four 2 mm bands.
        let mut gcode = String::from(";FLAVOR:Marlin\n;LAYER_COUNT:40\n");
        for layer in 0..40 {
            gcode.push_str(&format!(";LAYER:{layer}\nG0 F6000 X10 Y10 Z{:.2}\n", (layer + 1) as f64 * 0.2));
            gcode.push_str(";TYPE:WALL-OUTER\nG1 X20 Y20 E0.5\n");
        }
        let bands: Vec<Band> = (0..4)
            .map(|i| Band {
                index: i,
                z_mm: i as f64 * 2.0,
                temperature: 230.0 - 5.0 * i as f64,
            })
            .collect();
        let (out, injected) = step_temperatures(&gcode, &bands);
        assert_eq!(injected.len(), 4, "injections: {injected:?}");
        // Layer `l` is printed at (l + 1) * 0.2 mm, so the layer that first
        // reaches 2 mm is layer 9, not layer 10.
        let layers: Vec<usize> = injected.iter().map(|i| i.layer).collect();
        assert_eq!(layers, vec![0, 9, 19, 29]);
        let zs: Vec<f64> = injected.iter().map(|i| i.z_mm).collect();
        assert_eq!(zs, vec![0.2, 2.0, 4.0, 6.0]);
        let temps: Vec<f64> = injected.iter().map(|i| i.temperature).collect();
        assert_eq!(temps, vec![230.0, 225.0, 220.0, 215.0]);
        assert_eq!(out.matches("M104 S").count(), 4);
        assert!(out.contains(";SHALE-CALIBRATION band 3 from z=6.00 at 215C"));
        assert_eq!(out.matches(";LAYER:").count(), 40);
        // Nothing else in the file moved.
        let original: Vec<&str> = gcode.lines().filter(|l| l.starts_with("G1")).collect();
        let rewritten: Vec<&str> = out.lines().filter(|l| l.starts_with("G1")).collect();
        assert_eq!(original, rewritten);
    }

    #[test]
    fn a_slice_without_layers_is_left_alone() {
        let (out, injected) = step_temperatures(";FLAVOR:Marlin\nG28\n", &[]);
        assert_eq!(out, ";FLAVOR:Marlin\nG28\n");
        assert!(injected.is_empty());
    }
}
