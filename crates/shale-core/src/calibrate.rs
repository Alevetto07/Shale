//! Calibration prints.
//!
//! Bambu Studio's Calibration tab prints a part once per setting value and asks
//! you to pick the best band. Shale builds three: temperature, flow and
//! pressure advance. None of them needs a new engine feature — the geometry is
//! a stack of blocks or a tube, and the step between bands is one line written
//! into the slice (`M104`, `M221`, `M900`).
//!
//! The tower alternates a narrow block with a wider one, so every band meets
//! the same overhang on both sides — the difference between bands is then the
//! setting and nothing else. The flow coupon is a single-wall tube instead,
//! because flow is something you measure with calipers on a wall, not look at.

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
    /// The value this band is printed at: °C for the temperature tower,
    /// percent of flow for the coupon, K for pressure advance.
    pub value: f64,
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
    pub value: f64,
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
                value: start_c - step_c * i as f64,
            })
            .collect(),
        band_height_mm: band_height,
    }
}

/// A single-wall tube of `bands` bands, for measuring flow.
///
/// Flow is not something you look at; it is something you measure. A wall is
/// printed one extrusion wide, so the wall's thickness is the flow: at 100 %
/// the wall should caliper at the line width, and a band you can read is a band
/// you can set. The tube is 30 mm square with a 0.4 mm wall, which is one line
/// on a 0.4 mm nozzle — no infill, no top, nothing else to measure.
pub fn flow_coupon(
    center: [f64; 2],
    bands: usize,
    band_height: f64,
    start_pct: f64,
    step_pct: f64,
) -> Tower {
    let bands = bands.max(1);
    let band_height = if band_height > 0.0 {
        band_height
    } else {
        BAND_HEIGHT_MM
    };
    let side = 15.0;
    let wall = 0.4;
    // The walls run 0.2 mm into each other at the corners so the tube is one
    // solid rather than four boxes that merely touch.
    let reach = side - 0.2;
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    for i in 0..bands {
        let z0 = i as f64 * band_height;
        let z1 = z0 + band_height;
        for (min, max) in [
            ([center[0] - side, center[1] - side, z0], [center[0] + side, center[1] - side + wall, z1]),
            ([center[0] - side, center[1] + side - wall, z0], [center[0] + side, center[1] + side, z1]),
            ([center[0] - side, center[1] - reach, z0], [center[0] - side + wall, center[1] + reach, z1]),
            ([center[0] + side - wall, center[1] - reach, z0], [center[0] + side, center[1] + reach, z1]),
        ] {
            append_box(&mut mesh, min, max);
        }
    }
    Tower {
        mesh,
        bands: (0..bands)
            .map(|i| Band {
                index: i,
                z_mm: i as f64 * band_height,
                value: start_pct - step_pct * i as f64,
            })
            .collect(),
        band_height_mm: band_height,
    }
}

/// The tower again, but stepping pressure advance, which rises with height.
///
/// Pressure advance shows at corners: too little and the corner bulges, too
/// much and it is cut away. The alternating blocks give every band four
/// identical corners, so the band that reads best is the one to keep.
pub fn pressure_tower(
    center: [f64; 2],
    bands: usize,
    band_height: f64,
    start_k: f64,
    step_k: f64,
) -> Tower {
    let mut tower = temperature_tower(center, bands, band_height, start_k, step_k);
    for (i, band) in tower.bands.iter_mut().enumerate() {
        band.value = start_k + step_k * i as f64;
    }
    tower
}

/// The whole point of a temperature tower is that the print gets cooler as it
/// rises. A negative step would instead heat it up, which is a different test.
pub fn cooling_step(start_c: f64, step_c: f64, bands: usize) -> bool {
    step_c > 0.0 && start_c - step_c * bands.saturating_sub(1) as f64 > 0.0
}

/// What a calibration print steps from band to band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Hotend temperature, `M104` in whole °C.
    Temperature,
    /// Flow, `M221` in percent of the profile's flow.
    Flow,
    /// Pressure advance, `M900 K` in the firmware's own units.
    PressureAdvance,
}

impl Step {
    /// The line that sets this band's value. `M104` sets the target without
    /// waiting, so the first layer of a band prints while the hotend is still
    /// catching up; waiting (`M109`) would stop the print dead at every band.
    /// A ten millimetre band is time enough on an A1.
    pub fn line(&self, value: f64) -> String {
        match self {
            Step::Temperature => format!("M104 S{:.0}", value),
            // One decimal: a flow coupon is often stepped by 2.5 %.
            Step::Flow => format!("M221 S{:.1}", value),
            // Three decimals: pressure advance is stepped by thousandths.
            Step::PressureAdvance => format!("M900 K{:.3}", value),
        }
    }

    /// What the value is, for the banner and for the panel.
    pub fn unit(&self) -> &'static str {
        match self {
            Step::Temperature => "C",
            Step::Flow => "%",
            Step::PressureAdvance => "K",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Step::Temperature => "temperature-tower",
            Step::Flow => "flow-coupon",
            Step::PressureAdvance => "pressure-tower",
        }
    }
}

/// Rewrite a slice so each band is printed at its own temperature.
///
/// A thin wrapper over [`step_bands`], kept because most of the callers are
/// about temperature and reading `step_temperatures` says more than
/// `step_bands(.., Step::Temperature)`.
pub fn step_temperatures(gcode: &str, bands: &[Band]) -> (String, Vec<Injection>) {
    step_bands(gcode, bands, Step::Temperature)
}

/// Rewrite a slice so each band is printed at its own value of `step`.
pub fn step_bands(gcode: &str, bands: &[Band], step: Step) -> (String, Vec<Injection>) {
    if bands.is_empty() {
        return (gcode.to_string(), Vec::new());
    }
    // First pass: which height is each layer printed at? CuraEngine writes the
    // layer header, then the first move of the layer carries the Z.
    //
    // The end sequence is not a layer. It arrives after the last extrusion —
    // `G1 E-2 Z0.2` is a retract-and-drop, not the top of the print — so a Z
    // read after the last move that pushed filament would file the whole end
    // sequence into the top layer and step it back to band 0.
    let lines: Vec<&str> = gcode.lines().collect();
    let last_extrusion = lines
        .iter()
        .rposition(|line| extrusion(line).is_some())
        .unwrap_or(lines.len());
    let mut layer_z: Vec<f64> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index > last_extrusion {
            break;
        }
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
    banner.push_str(&format!(";SHALE-CALIBRATION {}\n", step.name()));
    for band in bands {
        banner.push_str(&format!(
            ";SHALE-CALIBRATION band {} from z={:.2} at {}{}\n",
            band.index,
            band.z_mm,
            band.value,
            step.unit()
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
                    if let Some(band) = bands.iter().find(|b| b.index == *band) {
                        out.push_str(&format!(
                            ";CALIBRATION band {} {}{}\n{} ; band {} at {}{}\n",
                            band.index,
                            band.value,
                            step.unit(),
                            step.line(band.value),
                            band.index,
                            band.value,
                            step.unit()
                        ));
                        injected.push(Injection {
                            band: band.index,
                            layer: index,
                            z_mm: layer_z.get(index).copied().unwrap_or(0.0),
                            value: band.value,
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

/// The E value a `G0`/`G1` line pushes, if it pushes any. A retract moves E
/// backwards, which is not an extrusion, so only a positive move counts.
fn extrusion(line: &str) -> Option<f64> {
    if !(line.starts_with("G0") || line.starts_with("G1")) {
        return None;
    }
    for token in line.split_whitespace() {
        if let Some(value) = token.strip_prefix('E') {
            if let Ok(e) = value.parse::<f64>() {
                if e > 0.0 {
                    return Some(e);
                }
            }
        }
    }
    None
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
        let temps: Vec<f64> = tower.bands.iter().map(|b| b.value).collect();
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
                value: 230.0 - 5.0 * i as f64,
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
        let temps: Vec<f64> = injected.iter().map(|i| i.value).collect();
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
    fn the_end_sequence_is_not_a_layer() {
        // Cura's end sequence retracts and drops the bed: `G1 E-2 Z0.2`. A
        // tower's last layer carries no Z of its own when the height has not
        // changed since the layer before it, so the first Z after the last
        // `;LAYER:` used to be that `Z0.2` — which filed the whole top of the
        // print into band 0 and dropped it 200 degrees.
        let gcode = "\
;FLAVOR:Marlin\n\
;LAYER:0\n\
G0 F6000 X10 Y10 Z0.20\n\
G1 F1800 X20 Y20 E0.5\n\
;LAYER:1\n\
G0 F6000 X10 Y10 Z5.20\n\
G1 F1800 X20 Y20 E1.0\n\
;LAYER:2\n\
G0 F6000 X10 Y10 Z10.20\n\
G1 F1800 X20 Y20 E1.5\n\
;LAYER:3\n\
G0 F6000 X10 Y10\n\
G1 F1800 X20 Y20 E2.0\n\
;TIME_ELAPSED:60.0\n\
G1 F1500 E0\n\
M140 S0\n\
M104 S0\n\
G1 E-2 Z0.2\n\
M84\n";
        let bands = vec![
            Band { index: 0, z_mm: 0.0, value: 230.0 },
            Band { index: 1, z_mm: 5.0, value: 220.0 },
            Band { index: 2, z_mm: 10.0, value: 210.0 },
        ];
        let (out, injected) = step_temperatures(gcode, &bands);
        // Band 2 starts at z=10, so it is written once, at the layer that first
        // reaches it; the layer above it needs no line because 210 still holds.
        let last = injected.last().expect("a band for every layer it reached");
        assert_eq!((last.layer, last.value), (2, 210.0), "injected: {injected:?}");
        // The temperature in force at the last layer is the top band's, not the
        // first one's: the end sequence's Z0.2 is not a height.
        let before_end = out.split(";TIME_ELAPSED").next().expect("an end");
        let in_force = before_end
            .lines()
            .rev()
            .find(|line| line.starts_with("M104 S"))
            .expect("a temperature");
        assert!(in_force.contains("210"), "the top layer ran at {in_force}");
        assert_eq!(out.matches("M104 S230").count(), 1, "out: {out}");
        assert!(out.contains(";SHALE-CALIBRATION band 2 from z=10.00 at 210C"));
    }

    #[test]
    fn the_flow_coupon_is_a_single_wall_tube() {
        let coupon = flow_coupon([128.0, 128.0], 5, 10.0, 105.0, 2.5);
        // Four walls per band, twelve triangles each.
        assert_eq!(coupon.mesh.triangles.len(), 5 * 4 * 12);
        let values: Vec<f64> = coupon.bands.iter().map(|b| b.value).collect();
        assert_eq!(values, vec![105.0, 102.5, 100.0, 97.5, 95.0]);
        // Every wall is one line wide on a 0.4 mm nozzle.
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for v in &coupon.mesh.vertices {
            lo = lo.min(v[0] as f64);
            hi = hi.max(v[0] as f64);
        }
        assert!((hi - lo - 30.0).abs() < 1e-6, "coupon is {} mm wide", hi - lo);
    }

    #[test]
    fn flow_and_pressure_are_written_with_their_own_lines() {
        let gcode = ";FLAVOR:Marlin\n;LAYER:0\nG0 F6000 X10 Y10 Z0.20\nG1 X20 Y20 E0.5\n";
        let bands = vec![
            Band { index: 0, z_mm: 0.0, value: 105.0 },
            Band { index: 1, z_mm: 3.0, value: 100.0 },
        ];
        let (flow, flow_injected) = step_bands(gcode, &bands, Step::Flow);
        assert_eq!(flow_injected.len(), 1);
        assert!(flow.contains("M221 S105.0"), "flow: {flow}");
        assert!(!flow.contains("M104"));

        let (pressure, _) = step_bands(gcode, &bands, Step::PressureAdvance);
        assert!(pressure.contains("M900 K105.000"), "pressure: {pressure}");
        assert!(!pressure.contains("M221"));
        assert!(pressure.contains(";SHALE-CALIBRATION pressure-tower"));
    }

    #[test]
    fn a_slice_without_layers_is_left_alone() {
        let (out, injected) = step_temperatures(";FLAVOR:Marlin\nG28\n", &[]);
        assert_eq!(out, ";FLAVOR:Marlin\nG28\n");
        assert!(injected.is_empty());
    }
}
