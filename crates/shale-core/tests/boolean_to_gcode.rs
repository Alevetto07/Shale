//! A part a Manifold boolean made, sliced by the real CuraEngine.
//!
//! The boolean tests stop at the mesh: they can say the volume is right and
//! every edge is shared. This one carries the same part the rest of the way —
//! through the resolver and into CuraEngine for the A1 profile — because the
//! question "does the new engine's output survive the slicer" is not answered
//! by a volume.
//!
//! What is asserted, and why each one would be a real failure:
//!
//! * the engine that answered is Manifold, so this is the new path;
//! * the G-code exists and is not empty, so the engine accepted the mesh;
//! * `;===== machine:` is in the first forty lines and **before** any command,
//!   which is the gate `shale-host` applies before it will send a file;
//! * the layer count is the height divided by the layer height — a boolean that
//!   left the part inside out or hollow would slice to something else;
//! * the filament length is positive and small, so the file has extrusion in it
//!   rather than travel moves around an empty part.
//!
//! It skips quietly when the engine or Cura's resources are not on the machine,
//! the same way `shale-mcp`'s real-engine test does: the suite has to be green
//! on a checkout with no Cura installed.

use shale_core::csg::{boolean, open_edges_topological, Engine, Op};
use shale_core::engine::CliEngine;
use shale_core::job::{self, PlacedModel};
use shale_core::mesh::{self, Transform};
use shale_core::primitives;
use shale_core::resolver::UserSettings;
use shale_core::{Catalog, ResourcePaths};

/// The A1, at the profile the end-to-end test in Batch 2 used.
const PRINTER: &str = "bambu_lab_a1";
const VARIANT: &str = "0.4 mm";
const QUALITY: &str = "0_20mm_standard_bbl_a1";
const MATERIAL: &str = "bambu_lab_pla_basic";

#[test]
fn a_boolean_part_slices_into_gcode_the_host_gate_accepts() {
    let configured = std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some();
    if !configured {
        return;
    }

    // The part the other tests already know the volume of: a 10 mm cube with a
    // 4 mm bore through it, 874.69 mm3 against 874.34 predicted.
    let plate = primitives::box_solid(10.0, 10.0, 10.0);
    let bore = mesh::bake(
        &primitives::cylinder(2.0, 14.0),
        &Transform {
            translation: [0.0, 0.0, -2.0],
            ..Transform::default()
        },
    );
    let (part, answered) = boolean(&plate, &bore, Op::Difference);
    assert_eq!(answered, Engine::Manifold, "the library should answer this");
    assert_eq!(open_edges_topological(&part), 0, "watertight before slicing");

    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available(), "CuraEngine did not answer help");

    let output = std::env::temp_dir()
        .join("shale")
        .join(format!("boolean-to-gcode-{}.gcode", std::process::id()));
    // Where the app puts a part on import: the plate's middle in Cura's own
    // frame. `to_engine_space` subtracts half the bed afterwards, so a part
    // handed in at the origin would be sliced 128 mm off the front-left corner
    // — which is exactly what the first run of this test did.
    let transform = mesh::place_on_bed(&part, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: "cube with a bore".to_string(),
        mesh: part,
        transform,
        settings: Vec::new(),
        slot: None,
    }];
    let outcome = job::slice(
        &catalog,
        &engine,
        PRINTER,
        Some(VARIANT.to_string()),
        Some(QUALITY.to_string()),
        &[Some(MATERIAL.to_string())],
        UserSettings {
            layer_height: Some(0.2),
            ..UserSettings::default()
        },
        &placed,
        &[],
        None,
        &[],
        output.clone(),
        &mut |_| {},
    )
    .expect("the engine should slice a Manifold part");

    let text = std::fs::read_to_string(&output).expect("the file the engine wrote");
    assert!(!text.is_empty(), "an empty file would prove nothing");

    // The gate, in the order the host applies it: the marker must be inside the
    // head the host reads, and nothing may command the machine before it.
    let head: Vec<&str> = text.lines().take(40).collect();
    let marker = head
        .iter()
        .position(|line| line.trim_start().starts_with(";===== machine:"))
        .unwrap_or_else(|| {
            panic!(
                "no `;===== machine:` in the first 40 lines, so the host would refuse this file:\n{}",
                head.join("\n")
            )
        });
    for (index, line) in head.iter().enumerate().take(marker) {
        let trimmed = line.trim_start();
        assert!(
            trimmed.is_empty() || trimmed.starts_with(';'),
            "line {index} commands the machine before the machine sequence: {line}"
        );
    }

    assert!(outcome.machine_gcode, "the A1's own start and end were used");

    // The header's bounds are Shale's own baked scene, and they say where the
    // part actually sat. A part handed in at the origin lands at -133..-123 —
    // 128 mm off the corner — which is what the first run of this test did
    // before it was given `place_on_bed`.
    let header_number = |prefix: &str| -> f64 {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(prefix))
            .unwrap_or_else(|| panic!("no {prefix} in the header"))
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{prefix} is not a number"))
    };
    let (min_x, max_x) = (header_number(";MINX:"), header_number(";MAXX:"));
    assert!(
        (min_x + 5.0).abs() < 0.6 && (max_x - 5.0).abs() < 0.6,
        "a 10 mm cube belongs at the middle of the plate, not x {min_x}..{max_x}"
    );

    let layers = outcome.toolpath.layers.len();
    // 10 mm at 0.2 mm is 50 layers; the engine may merge the top skin.
    assert!(
        (45..=55).contains(&layers),
        "10 mm at 0.2 mm should be about 50 layers, got {layers}"
    );
    assert!(
        outcome.filament_metres > 0.05 && outcome.filament_metres < 5.0,
        "a 10 mm cube with a bore is a metre or so of filament, got {}",
        outcome.filament_metres
    );
    let minutes = outcome
        .print_time_seconds
        .expect("the engine should estimate a time")
        / 60.0;
    assert!(
        (1.0..120.0).contains(&minutes),
        "a part this size is minutes, not hours: {minutes}"
    );

    eprintln!(
        "boolean part: {layers} layers, {:.3} m of filament, {:.1} min, {} bytes, machine header at line {}",
        outcome.filament_metres,
        minutes,
        text.len(),
        marker + 1
    );

    let _ = std::fs::remove_file(&output);
}

/// The brief's 30 000-triangle case, all the way to G-code.
///
/// The same subdivided box the watertightness test cuts, through the engine:
/// if Manifold's output were subtly wrong on a mesh this dense — coplanar
/// slivers, a long thin triangle, a vertex a fraction off the surface — the
/// slicer is where it would show, and this is the cheapest place to notice.
#[test]
fn a_thirty_thousand_triangle_part_also_slices() {
    let configured = std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some();
    if !configured {
        return;
    }

    let part = subdivided_box(10.0, 52);
    assert!(part.triangles.len() > 30_000, "the case the brief asked for");
    let bore = mesh::bake(
        &primitives::cylinder(2.0, 14.0),
        &Transform {
            translation: [0.0, 0.0, -2.0],
            ..Transform::default()
        },
    );
    let (cut, answered) = boolean(&part, &bore, Op::Difference);
    assert_eq!(answered, Engine::Manifold, "the library is the default path");
    assert_eq!(open_edges_topological(&cut), 0, "watertight before slicing");

    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available(), "CuraEngine did not answer help");
    let output = std::env::temp_dir()
        .join("shale")
        .join(format!("boolean-30k-{}.gcode", std::process::id()));
    let transform = mesh::place_on_bed(&cut, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: "thirty thousand".to_string(),
        mesh: cut,
        transform,
        settings: Vec::new(),
        slot: None,
    }];
    let outcome = job::slice(
        &catalog,
        &engine,
        PRINTER,
        Some(VARIANT.to_string()),
        Some(QUALITY.to_string()),
        &[Some(MATERIAL.to_string())],
        UserSettings {
            layer_height: Some(0.2),
            ..UserSettings::default()
        },
        &placed,
        &[],
        None,
        &[],
        output.clone(),
        &mut |_| {},
    )
    .expect("the engine should slice a 32 448-triangle part");

    let text = std::fs::read_to_string(&output).expect("the file the engine wrote");
    let head: Vec<&str> = text.lines().take(40).collect();
    let marker = head
        .iter()
        .position(|line| line.trim_start().starts_with(";===== machine:"))
        .unwrap_or_else(|| panic!("no machine marker in:\n{}", head.join("\n")));
    assert_eq!(marker, 13, "the marker is line 14, as for every A1 slice");
    assert!(
        outcome.toolpath.layers.len().abs_diff(50) <= 2,
        "10 mm at 0.2 mm should be about 50 layers, got {}",
        outcome.toolpath.layers.len()
    );
    assert!(
        outcome.filament_metres > 0.05 && outcome.filament_metres < 5.0,
        "a part this size is under a metre of filament, got {}",
        outcome.filament_metres
    );
    let minutes = outcome
        .print_time_seconds
        .expect("the engine's `;TIME:` line should carry an estimate")
        / 60.0;
    assert!(
        (1.0..120.0).contains(&minutes),
        "minutes, not the 6666 placeholder: {minutes}"
    );
    eprintln!(
        "30k part: {} layers, {:.3} m of filament, {minutes:.1} min, {} bytes",
        outcome.toolpath.layers.len(),
        outcome.filament_metres,
        text.len()
    );
    let _ = std::fs::remove_file(&output);
}

/// A box whose every face is a square grid: `12 · steps²` triangles.
///
/// The shape the watertightness test cuts, rebuilt here because that one lives
/// inside `csg.rs`'s test module and an integration test cannot see it. Winding
/// follows each face's own normal, which is the trap: index order is not
/// outward on all six.
fn subdivided_box(size: f64, steps: usize) -> shale_core::mesh::TriangleMesh {
    use shale_core::mesh::TriangleMesh;
    let half = size / 2.0;
    let mut mesh = TriangleMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    for axis in 0..3 {
        for side in [-half, half] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let mut grid: Vec<Vec<u32>> = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let mut row = Vec::with_capacity(steps + 1);
                for j in 0..=steps {
                    let mut point = [0.0; 3];
                    point[axis] = side;
                    point[u] = -half + size * (i as f64) / (steps as f64);
                    point[v] = -half + size * (j as f64) / (steps as f64);
                    mesh.vertices
                        .push([point[0] as f32, point[1] as f32, point[2] as f32]);
                    row.push((mesh.vertices.len() - 1) as u32);
                }
                grid.push(row);
            }
            let flip = side < 0.0;
            for i in 0..steps {
                for j in 0..steps {
                    let (a, b, c, d) = (
                        grid[i][j],
                        grid[i + 1][j],
                        grid[i + 1][j + 1],
                        grid[i][j + 1],
                    );
                    if flip {
                        mesh.triangles.push([a, c, b]);
                        mesh.triangles.push([a, d, c]);
                    } else {
                        mesh.triangles.push([a, b, c]);
                        mesh.triangles.push([a, c, d]);
                    }
                }
            }
        }
    }
    mesh
}
