// SPDX-License-Identifier: AGPL-3.0-or-later
//! Auto placement must engage on the T-wing, must stay inside the model, and
//! must tell the truth about flat overhangs (bridge-overhang fixture).
//!
//! Fixtures: `examples/t-wing.stl` (40x10x4 deck on a 10x10x20 pillar; the deck
//! top is the part's only upward-facing island) and `examples/bridge-overhang.stl`
//! (77x20x20: a 30 mm bridge plus a 15 mm flat cantilever at Z 15, and a
//! 40°-from-horizontal left overhang).

use std::path::PathBuf;

use shale_core::auto_spans::{analyse_for_nonplanar, spans_for_auto};
use shale_core::engine::CliEngine;
use shale_core::job::{self, PlacedModel};
use shale_core::mesh::{self, TriangleMesh};
use shale_core::resolver::UserSettings;
use shale_core::{Catalog, NonplanarMode, NonplanarOptions, ResourcePaths};
use shale_nonplanar::auto::PlannedSurfaceKind;

mod nonplanar_fixtures;

/// `scripts/nonplanar-e2e.ps1` sets SHALE_E2E_INPUT_DIR and asks for the
/// pillar-beside-dome fixture as an STL; nothing is written without it.
#[test]
fn export_e2e_inputs() {
    let Ok(dir) = std::env::var("SHALE_E2E_INPUT_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("e2e input dir");
    let pillar = nonplanar_fixtures::pillar_beside_dome();
    mesh::write_binary_stl(&dir.join("pillar-beside-dome.stl"), &pillar).expect("pillar stl");
}

const PRINTER: &str = "bambu_lab_a1";
const VARIANT: &str = "0.4 mm";
const QUALITY: &str = "0_20mm_standard_bbl_a1";
const MATERIAL: &str = "bambu_lab_pla_basic";

fn fixture(name: &str) -> TriangleMesh {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    mesh::load_stl(&bytes).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()))
}

fn baked(mesh: &TriangleMesh) -> (shale_nonplanar::mesh::Mesh, TriangleMesh) {
    let transform = mesh::place_on_bed(mesh, 256.0, 256.0, false);
    let baked = mesh::bake(mesh, &transform);
    let np = shale_nonplanar::mesh::Mesh {
        vertices: baked
            .vertices
            .iter()
            .map(|v| [v[0] as f64, v[1] as f64, v[2] as f64])
            .collect(),
        triangles: baked.triangles.clone(),
    };
    (np, baked)
}

fn opts() -> NonplanarOptions {
    NonplanarOptions {
        mode: NonplanarMode::Auto,
        max_angle_deg: 10.0,
        grid_step_mm: 0.5,
        ..NonplanarOptions::default()
    }
}

fn auto_plan(mesh: &TriangleMesh) -> shale_nonplanar::AutoPlacementReport {
    let (np, _) = baked(mesh);
    let spans = spans_for_auto(&analyse_for_nonplanar(mesh).spans);
    shale_nonplanar::plan_auto_placement(&np, &spans, &opts(), 0.2)
}

fn is_top_band(kind: PlannedSurfaceKind) -> bool {
    matches!(
        kind,
        PlannedSurfaceKind::CurvedTopBand | PlannedSurfaceKind::SlopedTopBand
    )
}

#[test]
fn t_wing_auto_enables_the_deck_top_band() {
    let mesh = fixture("t-wing.stl");
    let plan = auto_plan(&mesh);
    assert!(
        plan.enabled_surface_count > 0,
        "auto must engage on the T-wing: {:?}",
        plan.surfaces.iter().map(|s| &s.reason).collect::<Vec<_>>()
    );
    assert!(
        plan.surfaces
            .iter()
            .any(|s| s.enabled && is_top_band(s.kind)),
        "expected an enabled deck-top band: {:?}",
        plan.surfaces
    );
    assert!(!plan.fallback_to_planar);
}

#[test]
fn t_wing_auto_deform_stays_inside_the_model_envelope() {
    // Regression: the top band used to add its own tilt to `z_top_smooth`, which
    // printed the deck 1.5 mm above the model top (Z 25.48 on a 24.0 mm part) and
    // tripped the slope scan. The deform may compress the deck, never grow it.
    let mesh = fixture("t-wing.stl");
    let (np, _) = baked(&mesh);
    let spans = spans_for_auto(&analyse_for_nonplanar(&mesh).spans);
    let (deformed, ctx) = shale_nonplanar::prepare_pre_pass(&np, &opts(), 0.2, &spans);
    let plan = ctx.auto_placement.as_ref().expect("auto plan");
    assert!(plan.enabled_surface_count > 0);

    let model_top = np
        .vertices
        .iter()
        .map(|v| v[2])
        .fold(f64::NEG_INFINITY, f64::max);
    // The field's flat target is the printed envelope: it must never sit above
    // the model's own top. (Six subdivided vertices do come out 1.75 mm higher
    // in deform space, identically in off/curved-tops/auto — inherent to the
    // deform, and the post-pass maps the printed top back to 24.000.)
    assert!(
        ctx.field.deformed_top_z <= model_top + 0.05,
        "field top {:.3} above model top {model_top:.3}",
        ctx.field.deformed_top_z
    );

    // Vertices on the deck top must actually be spread by the deform.
    let deck: Vec<f64> = deformed
        .vertices
        .iter()
        .filter(|v| v[2] >= model_top - 4.0 && v[2] <= model_top + 0.05)
        .map(|v| v[2])
        .collect();
    let spread = deck.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - deck.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(spread > 0.2, "deck-top Z spread only {spread:.3} mm");
}

fn configured() -> bool {
    std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some()
        && std::env::var_os("SHALE_ORCA_DIR").is_some()
}

fn slice_with(mesh: &TriangleMesh, mode: NonplanarMode, tag: &str) -> String {
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    assert!(engine.available(), "CuraEngine not available");
    let mut settings = UserSettings {
        layer_height: Some(0.2),
        ..UserSettings::default()
    };
    settings.nonplanar = NonplanarOptions {
        mode,
        max_angle_deg: 10.0,
        grid_step_mm: 0.35,
        ..NonplanarOptions::default()
    };
    let path = std::env::temp_dir()
        .join("shale")
        .join(format!("auto-twing-{tag}-{}.gcode", std::process::id()));
    let transform = mesh::place_on_bed(mesh, 256.0, 256.0, false);
    let placed = vec![PlacedModel {
        name: "t-wing".into(),
        mesh: mesh.clone(),
        transform,
        settings: vec![("ironing_enabled".into(), "false".into())],
        slot: None,
    }];
    job::slice(
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
    std::fs::read_to_string(&path).expect("gcode")
}

#[test]
fn t_wing_auto_slice_prints_the_deck_top_non_planarly() {
    if !configured() {
        eprintln!("skipping: SHALE_CURAENGINE / resource paths not set");
        return;
    }
    let mesh = fixture("t-wing.stl");
    let planar = slice_with(&mesh, NonplanarMode::Off, "planar");
    let auto = slice_with(&mesh, NonplanarMode::Auto, "auto");

    let (pmin, pmax) = shale_nonplanar::top_layer_z_variation(&planar).expect("planar top layer");
    assert!(
        (pmax - pmin) <= 0.01,
        "planar top layer should be flat, got {pmin:.3}..{pmax:.3}"
    );
    let (amin, amax) = shale_nonplanar::top_layer_z_variation(&auto).expect("auto top layer");
    assert!(
        (amax - amin) > 0.2,
        "auto top layer Z spread only {:.3} mm ({amin:.3}..{amax:.3})",
        amax - amin
    );
    // The printed top may never rise above the model's own top surface.
    assert!(
        amax <= 24.05,
        "auto printed above the 24.0 mm model top: Z {amax:.3}"
    );
}

#[test]
fn bridge_overhang_detects_bridge_and_cantilever_and_declines_flat_tilt() {
    let mesh = fixture("bridge-overhang.stl");
    let (np, baked_mesh) = baked(&mesh);
    let analysis = analyse_for_nonplanar(&baked_mesh);
    assert!(
        analysis.spans.len() >= 2,
        "expected the bridge and the cantilever as spans, got {}",
        analysis.spans.len()
    );
    let horizontal = analysis
        .spans
        .iter()
        .filter(|s| (s.angle_from_vertical_deg - 90.0).abs() < 1.0)
        .count();
    assert_eq!(horizontal, 2, "expected two horizontal underside spans");

    let spans = spans_for_auto(&analysis.spans);
    let plan = shale_nonplanar::plan_auto_placement(&np, &spans, &opts(), 0.2);
    let underside: Vec<_> = plan
        .surfaces
        .iter()
        .filter(|s| !is_top_band(s.kind))
        .collect();
    assert_eq!(underside.len(), 2, "both undersides should be planned");
    for s in &underside {
        assert!(
            !s.enabled,
            "a 90° underside tilt round-trips to a planar toolpath, it must not be enabled: {}",
            s.reason
        );
        assert!(
            s.reason.contains("round-trips"),
            "reason should state the round-trip limit: {}",
            s.reason
        );
    }
    // Nothing else to gain: the top is flat and the 40° wall is cosmetic.
    assert_eq!(plan.enabled_surface_count, 0);
    assert!(plan.fallback_to_planar);
}
