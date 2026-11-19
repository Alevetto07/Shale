//! End-to-end non-planar curved tops vs planar baseline with CuraEngine 5.13.

//! Requires `scripts/fetch-resources.sh` and `scripts/fetch-curaengine.sh`.

mod nonplanar_fixtures;

use std::path::PathBuf;

use shale_core::engine::CliEngine;
use shale_core::job::{self, PlacedModel, SliceOutcome};
use shale_core::mesh::{self, Transform, TriangleMesh};
use shale_core::primitives;
use shale_core::resolver::UserSettings;

use nonplanar_fixtures::{
    merge_meshes as merge_fixture_meshes, pillar_beside_dome, pillar_beside_dome_parts,
    slice_pillar_beside_dome_sequential,
};
use shale_core::{Catalog, NonplanarMode, NonplanarOptions, ResourcePaths};

const PRINTER: &str = "bambu_lab_a1";
const VARIANT: &str = "0.4 mm";
const QUALITY: &str = "0_20mm_standard_bbl_a1";
const MATERIAL: &str = "bambu_lab_pla_basic";
const SHELL_LAYERS: usize = 8;

#[test]
fn benchy_body_not_in_top_band() {
    let mesh = benchy_like();
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let baked = mesh::bake(&mesh, &transform);
    let m = shale_nonplanar::mesh::Mesh {
        vertices: baked
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: baked.triangles.clone(),
    };
    let opts = NonplanarOptions {
        max_angle_deg: 10.0,
        grid_step_mm: 0.35,
        ..NonplanarOptions::default()
    };
    let field = shale_nonplanar::pipeline::rebuild_field(&m, &opts, 0.2);
    assert!(
        !field.gcode_needs_inverse(128.0, 128.0, 13.6),
        "benchy hull near mid height should skip inverse map"
    );
}

#[test]
fn auto_plan_flat_cube_no_surfaces() {
    let mesh = primitives::box_solid(30.0, 30.0, 5.0);
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let baked = mesh::bake(&mesh, &transform);
    let m = shale_nonplanar::mesh::Mesh {
        vertices: baked
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: baked.triangles.clone(),
    };
    let opts = NonplanarOptions {
        mode: NonplanarMode::Auto,
        max_angle_deg: 10.0,
        ..NonplanarOptions::default()
    };
    let plan = shale_nonplanar::plan_auto_placement(&m, &[], &opts, 0.2);
    assert_eq!(plan.enabled_surface_count, 0, "flat cube should stay planar");
    assert!(plan.fallback_to_planar);
}

#[test]
fn auto_plan_sphere_enables_curved_band() {
    let mesh = primitives::sphere(12.0);
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let baked = mesh::bake(&mesh, &transform);
    let m = shale_nonplanar::mesh::Mesh {
        vertices: baked
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: baked.triangles.clone(),
    };
    let opts = NonplanarOptions {
        mode: NonplanarMode::Auto,
        ..NonplanarOptions::default()
    };
    let analysis = shale_core::auto_spans::analyse_for_nonplanar(&baked);
    let spans = shale_core::auto_spans::spans_for_auto(&analysis.spans);
    let plan = shale_nonplanar::plan_auto_placement(&m, &spans, &opts, 0.2);
    assert!(
        plan.enabled_surface_count >= 1,
        "sphere dome should get a curved band"
    );
    assert!(
        plan.surfaces.len() <= 4,
        "dome should not fragment into many surfaces ({})",
        plan.surfaces.len()
    );
}

#[test]
fn chevron_height_field_detects_ramp() {
    let mesh = chevron_ramp();
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let baked = mesh::bake(&mesh, &transform);
    let m = shale_nonplanar::mesh::Mesh {
        vertices: baked
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: baked.triangles.clone(),
    };
    let field = deform_field_for(&mesh);
    let mut zs = Vec::new();
    for z in &field.grid.z_top_smooth {
        if let Some(v) = *z {
            zs.push(v);
        }
    }
    let spread = zs.iter().copied().fold(0.0f64, |acc, z| acc.max(z)) - zs
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    assert!(
        spread > 0.08,
        "chevron ramp height spread too small ({spread:.3} mm, {} samples)",
        zs.len()
    );
    let _ = m;
}

#[test]
fn t_wing_height_field_has_top_variation() {
    let mesh = mesh::load_mesh(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/t-wing.stl"),
    )
    .expect("t-wing stl");
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let baked = mesh::bake(&mesh, &transform);
    let m = shale_nonplanar::mesh::Mesh {
        vertices: baked
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: baked.triangles.clone(),
    };
    let opts = NonplanarOptions {
        max_angle_deg: 10.0,
        grid_step_mm: 0.35,
        ..NonplanarOptions::default()
    };
    let field = shale_nonplanar::pipeline::rebuild_field(&m, &opts, 0.2);
    let mut zs = Vec::new();
    for z in &field.grid.z_top_smooth {
        if let Some(v) = *z {
            zs.push(v);
        }
    }
    let z_min = zs.iter().copied().fold(f64::INFINITY, f64::min);
    let z_max = zs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let spread = z_max - z_min;
    assert!(
        spread > 0.5,
        "t-wing top height field spread too small ({spread:.3} mm, {} samples)",
        zs.len()
    );
}

fn configured() -> bool {
    std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some()
        && std::env::var_os("SHALE_ORCA_DIR").is_some()
}

fn artifact_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/nonplanar");
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn merge_meshes(a: &TriangleMesh, b: &TriangleMesh) -> TriangleMesh {
    let mut mesh = a.clone();
    let offset = mesh.vertices.len() as u32;
    mesh.vertices.extend(b.vertices.iter().copied());
    for tri in &b.triangles {
        mesh.triangles
            .push([tri[0] + offset, tri[1] + offset, tri[2] + offset]);
    }
    mesh
}

fn chevron_ramp() -> TriangleMesh {
    let tilt = 12.0f32.to_radians();
    let base_h = 10.0;
    let base = primitives::box_solid(40.0, 24.0, base_h);
    let mut slab = primitives::box_solid(40.0, 24.0, 1.2);
    for v in slab.vertices.iter_mut() {
        v[2] += v[0] * tilt.tan();
    }
    let slab = mesh::bake(
        &slab,
        &Transform {
            translation: [0.0, 0.0, base_h],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        },
    );
    merge_meshes(&base, &slab)
}

fn benchy_like() -> TriangleMesh {
    let base = primitives::box_solid(30.0, 30.0, 15.0);
    let knob = mesh::bake(
        &primitives::sphere(5.0),
        &Transform {
            translation: [0.0, 0.0, 15.0],
            rotation_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        },
    );
    merge_meshes(&base, &knob)
}

fn slice_pillar_beside_dome(nonplanar: bool, tag: &str) -> (String, SliceOutcome) {
    if !nonplanar {
        return slice_part("pillar-beside-dome", pillar_beside_dome(), NonplanarMode::Off, tag);
    }
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available());
    let path = std::env::temp_dir()
        .join("shale")
        .join(format!("nonplanar-{tag}-{}.gcode", std::process::id()));
    let (gcode, report) =
        slice_pillar_beside_dome_sequential(&catalog, &engine, PRINTER, tag);
    std::fs::write(artifact_dir().join(format!("{tag}.gcode")), &gcode).expect("artifact");
    std::fs::write(&path, &gcode).expect("gcode");
    let toolpath = shale_core::gcode::parse_toolpaths(&gcode);
    let outcome = SliceOutcome {
        gcode_path: path,
        resolved: catalog
            .resolve(&shale_core::resolver::ResolveRequest {
                printer_id: PRINTER.into(),
                variant_name: Some(VARIANT.into()),
                quality_type: Some(QUALITY.into()),
                material_id: Some(MATERIAL.into()),
                intent_category: None,
                settings: UserSettings::default(),
            })
            .expect("resolve"),
        toolpath,
        print_time_seconds: None,
        filament_metres: 0.0,
        flavour: shale_core::instructions::GcodeFlavour::Marlin,
        log_tail: String::new(),
        machine_gcode: true,
        nonplanar_report: Some(report),
        engine_gcode: None,
        planar_baseline_gcode: None,
    };
    (gcode, outcome)
}

fn slice_placed(
    name: &str,
    placed: Vec<PlacedModel>,
    mode: NonplanarMode,
    tag: &str,
) -> (String, SliceOutcome) {
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available());
    let mut settings = UserSettings {
        layer_height: Some(0.2),
        ..UserSettings::default()
    };
    settings.nonplanar = NonplanarOptions {
        mode,
        max_angle_deg: 10.0,
        ..NonplanarOptions::default()
    };
    let path = std::env::temp_dir()
        .join("shale")
        .join(format!("nonplanar-{tag}-{}.gcode", std::process::id()));
    let outcome = job::slice(
        &catalog,
        &engine,
        PRINTER,
        Some(VARIANT.into()),
        Some(QUALITY.into()),
        &[Some(MATERIAL.into())],
        settings,
        &placed,
        &[],
        None,
        &[],
        path.clone(),
        &mut |_| {},
    )
    .expect("slice");
    let text = std::fs::read_to_string(&path).expect("gcode");
    std::fs::write(artifact_dir().join(format!("{tag}.gcode")), &text).expect("artifact gcode");
    let _ = name;
    (text, outcome)
}

fn slice_part(
    name: &str,
    mesh: TriangleMesh,
    mode: NonplanarMode,
    tag: &str,
) -> (String, SliceOutcome) {
    let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: name.into(),
        mesh,
        transform,
        settings: vec![("ironing_enabled".into(), "false".into())],
        slot: None,
    }];
    slice_placed(name, placed, mode, tag)
}

fn run_python(script: &str, args: &[&str]) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts").join(script);
    if path.is_file() {
        let _ = std::process::Command::new("python3")
            .arg(&path)
            .args(args)
            .status();
    }
}

fn plot_artifacts(tag: &str, title: &str) {
    let gcode = artifact_dir().join(format!("{tag}.gcode"));
    run_python(
        "plot-nonplanar-toplayers.py",
        &[
            gcode.to_str().unwrap(),
            "-o",
            artifact_dir()
                .join(format!("{tag}-toplayer.png"))
                .to_str()
                .unwrap(),
            "--title",
            title,
        ],
    );
    run_python(
        "plot-nonplanar-xz-section.py",
        &[
            gcode.to_str().unwrap(),
            "-o",
            artifact_dir()
                .join(format!("{tag}-xz.png"))
                .to_str()
                .unwrap(),
            "--layers",
            &SHELL_LAYERS.to_string(),
            "--title",
            &format!("{title} — XZ section"),
        ],
    );
    run_python(
        "plot-nonplanar-slope-hist.py",
        &[
            gcode.to_str().unwrap(),
            "-o",
            artifact_dir()
                .join(format!("{tag}-slope-hist.png"))
                .to_str()
                .unwrap(),
            "--layers",
            &SHELL_LAYERS.to_string(),
            "--title",
            &format!("{title} — in-layer slope"),
        ],
    );
}

#[derive(serde::Serialize)]
struct CaseMetrics {
    name: String,
    planar_layers: usize,
    curved_layers: usize,
    planar_time_s: Option<f64>,
    curved_time_s: Option<f64>,
    planar_top_z_min: f64,
    planar_top_z_max: f64,
    curved_top_z_min: f64,
    curved_top_z_max: f64,
    planar_in_layer_max_slope_deg: f64,
    curved_in_layer_max_slope_deg: f64,
    curved_max_slope_across_layers_deg: f64,
    curved_slope_histogram: Vec<(f64, usize)>,
    curved_band_max_slope_deg: Option<f64>,
    max_angle_ok: bool,
    envelope_ok: bool,
    collision_notes: Vec<String>,
    planar_equivalence_ok: bool,
    safety_ok: Option<bool>,
    collision_fixes: usize,
}

fn nonplanar_opts() -> NonplanarOptions {
    NonplanarOptions {
        max_angle_deg: 10.0,
        grid_step_mm: 0.35,
        ..NonplanarOptions::default()
    }
}

fn deform_field_for(mesh: &TriangleMesh) -> shale_nonplanar::deform::DeformField {
    let transform = mesh::place_on_bed(mesh, 256.0, 256.0, false);
    let baked = mesh::bake(mesh, &transform);
    let m = shale_nonplanar::mesh::Mesh {
        vertices: baked
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: baked.triangles.clone(),
    };
    shale_nonplanar::pipeline::rebuild_field(&m, &nonplanar_opts(), 0.2)
}

fn global_band_floor(mesh: &TriangleMesh) -> f64 {
    deform_field_for(mesh).global_band_floor_z()
}

fn analyse(
    name: &str,
    planar: &SliceOutcome,
    planar_gcode: &str,
    curved_gcode: &str,
    curved: &SliceOutcome,
    field_mesh: &TriangleMesh,
) -> CaseMetrics {
    let field = deform_field_for(field_mesh);
    let (pz0, pz1) =
        shale_nonplanar::top_shell_z_variation(planar_gcode, SHELL_LAYERS).unwrap_or((0.0, 0.0));
    let (cz0, cz1) =
        shale_nonplanar::top_shell_z_variation(curved_gcode, SHELL_LAYERS).unwrap_or((0.0, 0.0));
    CaseMetrics {
        name: name.to_string(),
        planar_layers: planar.toolpath.layers.len(),
        curved_layers: curved.toolpath.layers.len(),
        planar_time_s: planar.print_time_seconds,
        curved_time_s: curved.print_time_seconds,
        planar_top_z_min: pz0,
        planar_top_z_max: pz1,
        curved_top_z_min: cz0,
        curved_top_z_max: cz1,
        planar_in_layer_max_slope_deg: shale_nonplanar::max_top_shell_slope_deg(
            planar_gcode,
            SHELL_LAYERS,
        )
        .unwrap_or(0.0),
        curved_in_layer_max_slope_deg: shale_nonplanar::max_top_shell_slope_deg(
            curved_gcode,
            SHELL_LAYERS,
        )
        .unwrap_or(0.0),
        curved_max_slope_across_layers_deg: shale_nonplanar::max_top_shell_slope_across_layers_deg(
            curved_gcode,
            SHELL_LAYERS,
        )
        .unwrap_or(0.0),
        curved_slope_histogram: shale_nonplanar::in_layer_slope_histogram(
            curved_gcode,
            SHELL_LAYERS,
            0.5,
        ),
        curved_band_max_slope_deg: shale_nonplanar::max_extrusion_slope_above_z(
            curved_gcode,
            field.global_band_floor_z(),
        ),
        max_angle_ok: shale_nonplanar::max_extrusion_slope_above_z(
            curved_gcode,
            field.global_band_floor_z(),
        )
        .is_none_or(|d| d <= 10.5),
        envelope_ok: shale_nonplanar::gcode_respects_envelope(curved_gcode, PRINTER, 10.0),
        collision_notes: curved
            .nonplanar_report
            .as_ref()
            .map(|r| r.notes.clone())
            .unwrap_or_default(),
        planar_equivalence_ok: curved
            .nonplanar_report
            .as_ref()
            .is_some_and(|r| r.planar_equivalence_ok),
        safety_ok: curved
            .nonplanar_report
            .as_ref()
            .and_then(|r| r.safety.as_ref().map(|s| s.ok)),
        collision_fixes: curved
            .nonplanar_report
            .as_ref()
            .map(|r| r.collision_adjustments.len())
            .unwrap_or(0),
    }
}

#[test]
fn auto_mode_slices_with_placement_report() {
    if !configured() {
        return;
    }
    let mesh = primitives::sphere(12.0);
    let (_gcode, outcome) = slice_part("dome-auto", mesh, NonplanarMode::Auto, "dome-auto");
    let report = outcome
        .nonplanar_report
        .as_ref()
        .expect("auto slice report");
    let auto = report
        .auto_placement
        .as_ref()
        .expect("auto placement block");
    assert!(auto.enabled_surface_count >= 1);
    assert!(
        auto.surfaces.len() <= 6,
        "merged surfaces expected, got {}",
        auto.surfaces.len()
    );
    assert_eq!(report.mode, "auto");
}

#[test]
fn nonplanar_end_to_end_benchmarks() {
    if !configured() {
        return;
    }

    let cases: Vec<(&str, TriangleMesh)> = vec![
        ("dome", primitives::sphere(12.0)),
        ("chevron", chevron_ramp()),
        (
            "t-wing",
            mesh::load_mesh(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/t-wing.stl"))
                .expect("t-wing stl"),
        ),
        ("benchy", benchy_like()),
        (
            "pillar-beside-dome",
            primitives::sphere(0.1), // built as two parts in loop
        ),
    ];

    let mut metrics = Vec::new();
    for (name, mesh) in cases {
        let mesh_for_field = if name == "pillar-beside-dome" {
            let (dome_mesh, _, pillar_mesh, _) = pillar_beside_dome_parts();
            merge_fixture_meshes(&dome_mesh, &pillar_mesh)
        } else {
            mesh.clone()
        };
        let (planar_gcode, planar_out, curved_gcode, curved_out) = if name == "pillar-beside-dome" {
            let (pg, po) = slice_pillar_beside_dome(false, &format!("{name}-planar"));
            let (cg, co) = slice_pillar_beside_dome(true, &format!("{name}-curved"));
            (pg, po, cg, co)
        } else {
            let (pg, po) =
                slice_part(name, mesh.clone(), NonplanarMode::Off, &format!("{name}-planar"));
            let (cg, co) =
                slice_part(name, mesh, NonplanarMode::CurvedTops, &format!("{name}-curved"));
            (pg, po, cg, co)
        };
        assert!(curved_out.nonplanar_report.is_some());
        assert!(!curved_gcode.contains(";TYPE:IRONING"));

        let report = curved_out.nonplanar_report.as_ref().unwrap();
        assert!(
            report.safety.is_some(),
            "{name}: expected machine-readable safety report"
        );
        if name != "pillar-beside-dome" {
            assert!(
                report.planar_equivalence_ok,
                "{name}: below-band G-code must match planar slice"
            );
        }
        assert!(
            report
                .safety
                .as_ref()
                .is_some_and(|s| s.ok),
            "{name}: safety violations: {:?}",
            report.safety.as_ref().map(|s| &s.violations)
        );

        let pslope =
            shale_nonplanar::max_top_shell_slope_deg(&planar_gcode, SHELL_LAYERS).unwrap_or(0.0);
        let cslope =
            shale_nonplanar::max_top_shell_slope_deg(&curved_gcode, SHELL_LAYERS).unwrap_or(0.0);
        let cslope_layers =
            shale_nonplanar::max_top_shell_slope_across_layers_deg(&curved_gcode, SHELL_LAYERS)
                .unwrap_or(0.0);
        let band_z = global_band_floor(&mesh_for_field);
        let band_max = shale_nonplanar::max_extrusion_slope_above_z(&curved_gcode, band_z)
            .unwrap_or(0.0);
        let slope_cap = report
            .safety
            .as_ref()
            .map(|s| s.slope_cap_deg)
            .unwrap_or(10.0 + shale_nonplanar::SLOPE_CAP_TOLERANCE_DEG);
        assert!(
            band_max <= slope_cap + 1e-3,
            "{name}: extrusions at or above band z={band_z:.3} must respect {slope_cap:.2}° cap (saw {band_max:.2}°)"
        );

        let (pz0, pz1) =
            shale_nonplanar::top_shell_z_variation(&planar_gcode, SHELL_LAYERS).unwrap();
        let (cz0, cz1) =
            shale_nonplanar::top_shell_z_variation(&curved_gcode, SHELL_LAYERS).unwrap();

        match name {
            "dome" => {
                assert!(
                    (7.0..=10.5).contains(&cslope),
                    "dome in-layer slope {cslope:.2}° expected near 10° cap ({:.1}° incl. tolerance)",
                    10.0 + shale_nonplanar::SLOPE_CAP_TOLERANCE_DEG
                );
                assert!(
                    cslope_layers <= 10.0 + shale_nonplanar::SLOPE_CAP_TOLERANCE_DEG,
                    "dome across-layer slope {cslope_layers:.2}° must be ≤{:.1}° (10° + {}° tolerance)",
                    10.0 + shale_nonplanar::SLOPE_CAP_TOLERANCE_DEG,
                    shale_nonplanar::SLOPE_CAP_TOLERANCE_DEG
                );
                assert!(cslope > pslope + 2.0);
            }
            "chevron" => {
                assert!(pslope < 1.0, "planar chevron should be stepped ({pslope:.2}°)");
                let cspread = cz1 - cz0;
                assert!(
                    cspread > 0.05,
                    "chevron curved top shell Z spread {cspread:.3} mm"
                );
                let slope_cap = 10.0 + shale_nonplanar::SLOPE_CAP_TOLERANCE_DEG;
                assert!(
                    (5.0..=slope_cap).contains(&band_max),
                    "chevron band slope {band_max:.2}° expected near 10° cap ({slope_cap:.1}°)"
                );
                assert!(
                    cslope_layers <= slope_cap,
                    "chevron shell slope {cslope_layers:.2}° must be ≤{slope_cap:.1}°"
                );
                assert!(band_max > pslope + 0.5, "chevron should beat planar staircase");
            }
            "t-wing" => {
                assert!(
                    cslope_layers > pslope + 0.4
                        || cslope > pslope + 0.4
                        || (cz1 - cz0) > (pz1 - pz0) + 0.05,
                    "t-wing should gain curvature (slope {cslope:.2}/{cslope_layers:.2}°, spread {:.3} vs {:.3})",
                    cz1 - cz0,
                    pz1 - pz0
                );
            }
            "benchy" => {}
            "pillar-beside-dome" => {
                assert!(
                    !report.collision_adjustments.is_empty(),
                    "pillar beside dome: expected collision flattening, got {:?}",
                    report.notes
                );
                assert!(
                    report
                        .safety
                        .as_ref()
                        .is_some_and(|s| s.travel_collisions_remaining == 0),
                    "final G-code must have no travel collisions"
                );
            }
            _ => {}
        }

        plot_artifacts(&format!("{name}-curved"), &format!("{name} curved tops"));
        plot_artifacts(&format!("{name}-planar"), &format!("{name} planar tops"));

        metrics.push(analyse(
            name,
            &planar_out,
            &planar_gcode,
            &curved_gcode,
            &curved_out,
            &mesh_for_field,
        ));
    }

    let report = serde_json::to_string_pretty(&metrics).expect("json");
    std::fs::write(artifact_dir().join("measurements.json"), report).expect("metrics file");
}