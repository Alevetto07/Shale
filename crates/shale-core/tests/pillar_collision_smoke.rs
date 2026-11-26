//! Tune `pillar_beside_dome_parts` until real slices trigger envelope flattening.

mod nonplanar_fixtures;

use nonplanar_fixtures::{slice_pillar_beside_dome_sequential, slice_pillar_beside_dome_with_mode};

use shale_core::engine::CliEngine;
use shale_core::{Catalog, NonplanarMode, ResourcePaths};
use std::time::{Duration, Instant};

/// Wall-clock budget for one `auto` slice of the pillar + dome plate.
///
/// A curved pass that cannot be decided must come back with a named fallback, not
/// spin: the limiter is bounded by pass count and the post-pass by the collision
/// sweep, so 120 s is two orders of magnitude above the measured 5-8 s and only
/// catches a real hang (an unbounded limiter, a replanning loop).
const AUTO_SLICE_BUDGET: Duration = Duration::from_secs(120);

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

#[test]
fn pillar_beside_dome_auto_returns_within_its_budget() {
    if !configured() {
        return;
    }
    let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
    let engine = CliEngine::from_env();
    let started = Instant::now();
    let (gcode, report) = slice_pillar_beside_dome_with_mode(
        &catalog,
        &engine,
        "bambu_lab_a1",
        "auto-bound",
        NonplanarMode::Auto,
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed < AUTO_SLICE_BUDGET,
        "auto must return within {AUTO_SLICE_BUDGET:?}, took {elapsed:?}"
    );
    assert!(!gcode.is_empty(), "auto slice produced no G-code");
    // Curved-safe, or a fallback that names its cause. Silence is the one outcome
    // that is not allowed.
    if report.fallback_to_planar {
        assert!(
            !report.fallback_reasons.is_empty(),
            "a planar fallback must name its reason"
        );
    } else {
        let safety = report
            .safety
            .as_ref()
            .expect("a kept curved slice carries a safety report");
        assert!(
            safety.ok && safety.travel_collisions_remaining == 0 && safety.slope_within_cap,
            "a kept curved slice must be clean: {safety:?}"
        );
    }
    eprintln!(
        "pillar-beside-dome auto: {elapsed:?}, fallback={}",
        report.fallback_to_planar
    );
}
