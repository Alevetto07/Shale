//! Tune `pillar_beside_dome_parts` until real slices trigger envelope flattening.

mod nonplanar_fixtures;

use nonplanar_fixtures::slice_pillar_beside_dome_sequential;

use shale_core::engine::CliEngine;
use shale_core::{Catalog, ResourcePaths};

fn configured() -> bool {
    std::env::var_os("SHALE_CURAENGINE").is_some()
        && std::env::var_os("SHALE_CURA_DIR").is_some()
        && std::env::var_os("SHALE_MATERIALS_DIR").is_some()
        && std::env::var_os("SHALE_ORCA_DIR").is_some()
}

#[test]
fn sliced_pillar_beside_dome_is_clean_or_falls_back_with_a_collision_reason() {
    if !configured() {
        return;
    }
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    let (_, report) =
        slice_pillar_beside_dome_sequential(&catalog, &engine, "bambu_lab_a1", "smoke");
    assert!(
        report.collision_adjustments.is_empty(),
        "the post-pass must not edit the field after slicing: {:?}",
        report.collision_adjustments
    );
    if report.fallback_to_planar {
        assert!(
            report.fallback_reasons.iter().any(|r| {
                let r = r.to_lowercase();
                r.contains("collision") || r.contains("slope")
            }),
            "a fallback must name its cause (collision or slope): {:?}",
            report.fallback_reasons
        );
    } else {
        let safety = report
            .safety
            .as_ref()
            .expect("a kept curved slice carries a safety report");
        assert!(
            safety.ok,
            "a kept curved slice must be clean: {:?}",
            safety.violations
        );
        assert_eq!(
            safety.travel_collisions_remaining, 0,
            "a kept curved slice must have no travel collisions"
        );
        assert!(safety.slope_within_cap, "{safety:?}");
        assert!(safety.z_within_limits, "{safety:?}");
    }
}
