#[cfg(test)]
mod auto_placement {
    use crate::auto::plan_auto_placement;
    use crate::auto::types::SupportSpanInput;
    use crate::config::{NonplanarMode, NonplanarOptions};
    use crate::height_field::build_height_field;
    use crate::test_meshes::{box_mesh, bridge_mesh, cantilever_mesh, sphere_mesh, tilted_top_slab};

    fn opts() -> NonplanarOptions {
        NonplanarOptions {
            mode: NonplanarMode::Auto,
            max_angle_deg: 10.0,
            grid_step_mm: 0.5,
            ..NonplanarOptions::default()
        }
    }

    fn mock_bridge_span() -> SupportSpanInput {
        SupportSpanInput {
            kind: "bridge".into(),
            length_mm: 24.0,
            width_mm: 8.0,
            area_mm2: 120.0,
            z_mm: 21.0,
            angle_from_vertical_deg: 88.0,
            anchor_ends: 2,
            sample_min: [8.0, -4.0, 20.5],
            sample_max: [32.0, 4.0, 21.0],
        }
    }

    #[test]
    fn flat_plate_has_no_enabled_surfaces() {
        let mesh = box_mesh(40.0, 40.0, 4.0);
        let plan = plan_auto_placement(&mesh, &[], &opts(), 0.2);
        assert_eq!(plan.enabled_surface_count, 0);
        assert!(plan.fallback_to_planar);
    }

    #[test]
    fn dome_is_one_coherent_top_surface() {
        let mesh = sphere_mesh(12.0);
        let plan = plan_auto_placement(&mesh, &[], &opts(), 0.2);
        let top_surfaces: Vec<_> = plan
            .surfaces
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    crate::auto::PlannedSurfaceKind::CurvedTopBand
                        | crate::auto::PlannedSurfaceKind::SlopedTopBand
                )
            })
            .collect();
        assert!(
            top_surfaces.len() <= 3,
            "dome should merge to few surfaces, got {}",
            top_surfaces.len()
        );
        assert!(plan.enabled_surface_count >= 1);
    }

    #[test]
    fn overhang_ramp_enables_sloped_band() {
        let mesh = tilted_top_slab(36.0, 20.0, 1.2, 9.0);
        let plan = plan_auto_placement(&mesh, &[], &opts(), 0.2);
        assert!(plan.enabled_surface_count >= 1, "{:?}", plan.surfaces);
        let grid = build_height_field(&mesh, 0.5, 10.0, opts().effective_band_thickness(0.2));
        let spread: f64 = grid
            .z_top_smooth
            .iter()
            .flatten()
            .fold(f64::NEG_INFINITY, |a, &z| a.max(z))
            - grid
                .z_top_smooth
                .iter()
                .flatten()
                .fold(f64::INFINITY, |a, &z| a.min(z));
        assert!(spread > 1.0, "expected height variation on ramp ({spread:.2} mm)");
    }

    #[test]
    fn bridge_span_plans_tilt_with_overhang_delta() {
        let mesh = bridge_mesh();
        let plan = plan_auto_placement(&mesh, &[mock_bridge_span()], &opts(), 0.2);
        let bridge_surface = plan
            .surfaces
            .iter()
            .find(|s| s.kind == crate::auto::PlannedSurfaceKind::BridgeTiltBand);
        assert!(bridge_surface.is_some(), "expected bridge tilt band");
        let s = bridge_surface.unwrap();
        assert!(s.enabled);
        assert!(s.overhang_angle_before_deg.unwrap() > s.overhang_angle_after_deg.unwrap());
    }

    #[test]
    fn cantilever_from_supports_analysis() {
        let mesh = cantilever_mesh();
        let span = SupportSpanInput {
            kind: "cantilever".into(),
            length_mm: 30.0,
            width_mm: 8.0,
            area_mm2: 200.0,
            z_mm: 16.0,
            angle_from_vertical_deg: 85.0,
            anchor_ends: 1,
            sample_min: [10.0, -4.0, 15.0],
            sample_max: [40.0, 4.0, 16.5],
        };
        let plan = plan_auto_placement(&mesh, &[span], &opts(), 0.2);
        assert!(
            plan.surfaces
                .iter()
                .any(|s| s.kind == crate::auto::PlannedSurfaceKind::CantileverTiltBand && s.enabled)
        );
    }
}