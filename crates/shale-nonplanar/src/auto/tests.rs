#[cfg(test)]
mod auto_placement {
    use crate::auto::{plan_auto_placement, DetectedFeatureKind};
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

    #[test]
    fn flat_plate_has_no_enabled_surfaces() {
        let mesh = box_mesh(40.0, 40.0, 4.0);
        let plan = plan_auto_placement(&mesh, &opts(), 0.2);
        assert_eq!(plan.enabled_surface_count, 0);
        assert!(plan.fallback_to_planar);
        assert!(plan
            .detection
            .features
            .iter()
            .any(|f| f.kind == DetectedFeatureKind::FlatTop));
    }

    #[test]
    fn dome_gets_curved_top_band() {
        let mesh = sphere_mesh(12.0);
        let plan = plan_auto_placement(&mesh, &opts(), 0.2);
        assert!(plan.enabled_surface_count >= 1);
        assert!(plan
            .surfaces
            .iter()
            .any(|s| s.enabled && s.tilt_deg <= 10.0 + 0.5));
    }

    #[test]
    fn overhang_ramp_enables_sloped_band() {
        let mesh = tilted_top_slab(36.0, 20.0, 1.2, 9.0);
        let plan = plan_auto_placement(&mesh, &opts(), 0.2);
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
    fn bridge_is_detected_without_enabling_surface() {
        let mesh = bridge_mesh();
        let plan = plan_auto_placement(&mesh, &opts(), 0.2);
        assert!(
            plan.detection.features.iter().any(|f| {
                matches!(
                    f.kind,
                    DetectedFeatureKind::Bridge
                        | DetectedFeatureKind::Island
                        | DetectedFeatureKind::Overhang
                )
            }),
            "expected an underside span on the bridge mesh"
        );
        assert!(
            !plan.surfaces.iter().any(|s| s.enabled),
            "bridges should not activate curved-top bands"
        );
    }

    #[test]
    fn cantilever_is_detected() {
        let mesh = cantilever_mesh();
        let plan = plan_auto_placement(&mesh, &opts(), 0.2);
        assert!(
            plan.detection.features.iter().any(|f| {
                matches!(
                    f.kind,
                    DetectedFeatureKind::Cantilever
                        | DetectedFeatureKind::Overhang
                        | DetectedFeatureKind::Bridge
                )
            }),
            "expected an underside feature on the cantilever mesh"
        );
    }
}