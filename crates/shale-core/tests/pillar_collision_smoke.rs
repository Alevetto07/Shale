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
fn sliced_pillar_beside_dome_triggers_collision_flattening() {
    if !configured() {
        return;
    }
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    let (_, report) =
        slice_pillar_beside_dome_sequential(&catalog, &engine, "bambu_lab_a1", "smoke");
    let n = report.collision_adjustments.len();
    assert!(
        n > 0,
        "expected conservative A1 envelope hits on pillar fixture, got 0; notes={:?}",
        report.notes
    );
    assert!(
        report.safety.as_ref().is_some_and(|s| s.ok),
        "safety must stay ok after flattening: travel_collisions={:?} violations={:?}",
        report
            .safety
            .as_ref()
            .map(|s| s.travel_collisions_remaining),
        report
            .safety
            .as_ref()
            .map(|s| &s.violations)
    );
}