//! Shelf STL must yield a non-cosmetic underside span for auto placement.

use std::path::PathBuf;

use shale_core::auto_spans::{analyse_for_nonplanar, spans_for_auto};
use shale_core::mesh::{load_mesh, signed_volume};
use shale_core::supports::FeatureKind;
use shale_nonplanar::{plan_auto_placement, NonplanarMode, NonplanarOptions};

#[test]
fn overhang_shelf_mesh_yields_cantilever_band() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/overhang-shelf.stl");
    let mesh = load_mesh(&path).expect("overhang shelf stl");
    assert!(
        signed_volume(&mesh) > 0.0,
        "shelf mesh must be outward-wound (signed volume {})",
        signed_volume(&mesh)
    );
    let analysis = analyse_for_nonplanar(&mesh);
    let steep: Vec<_> = analysis
        .spans
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                FeatureKind::Cantilever
                    | FeatureKind::Overhang
                    | FeatureKind::Island
                    | FeatureKind::Bridge
            ) && s.angle_from_vertical_deg >= 70.0
        })
        .collect();
    assert!(
        !steep.is_empty(),
        "expected steep underside span, got {:?}",
        analysis
            .spans
            .iter()
            .map(|s| (s.kind, s.angle_from_vertical_deg, s.area_mm2))
            .collect::<Vec<_>>()
    );
    let spans = spans_for_auto(&analysis.spans);
    let opts = NonplanarOptions {
        mode: NonplanarMode::Auto,
        max_angle_deg: 10.0,
        ..NonplanarOptions::default()
    };
    let plan = plan_auto_placement(
        &shale_core::nonplanar_bridge::to_nonplanar(&mesh),
        &spans,
        &opts,
        0.2,
    );
    assert!(
        plan.enabled_surface_count >= 1,
        "shelf should enable at least one tilt band: {:?}",
        plan
    );
}