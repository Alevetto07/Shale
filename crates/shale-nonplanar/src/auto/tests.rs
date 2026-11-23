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
    fn underside_band_tilts_bridge_bottom_in_mesh() {
        use crate::pipeline::{deform_model, prepare_plate};

        let mesh = bridge_mesh();
        let ctx = prepare_plate(&mesh, &opts(), 0.2, "bambu_lab_a1", &[mock_bridge_span()])
            .expect("context");
        let deformed = deform_model(&mesh, &ctx);
        let auto = ctx.auto_placement.as_ref().expect("auto plan");
        assert!(auto.enabled_surface_count >= 1);
        let zs: Vec<f64> = deformed
            .vertices
            .iter()
            .filter(|v| {
                v[0] >= 9.0
                    && v[0] <= 31.0
                    && v[1].abs() <= 4.5
                    && (v[2] - 21.0).abs() < 1.5
            })
            .map(|v| v[2])
            .collect();
        assert!(zs.len() >= 8, "expected bridge slab verts, got {}", zs.len());
        let spread = zs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - zs.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            spread > 0.08,
            "underside tilt should spread Z by >0.08 mm (saw {spread:.3} mm, n={})",
            zs.len()
        );
    }

    #[test]
    fn enabled_underside_band_produces_measurable_gcode_tilt() {
        use crate::auto::PlannedSurfaceKind;
        use crate::pipeline::apply_post_pass;
        use crate::pipeline::max_underside_band_slope_deg;

        let mesh = bridge_mesh();
        let spans = [mock_bridge_span()];
        let opts = opts();
        let plan = plan_auto_placement(&mesh, &spans, &opts, 0.2);
        let surface = plan
            .surfaces
            .iter()
            .find(|s| s.kind == PlannedSurfaceKind::BridgeTiltBand && s.enabled)
            .expect("enabled bridge band");
        let layer_h = 0.2;
        // The G-code must be generated from the same field the post-pass uses.
        let ctx = crate::pipeline::prepare_plate(&mesh, &opts, layer_h, "bambu_lab_a1", &spans)
            .expect("context");
        let mut planar = String::from(";TYPE:SKIN\nG1 F3000 X10 Y0 Z21.0 E0\n");
        for (i, x) in [10.0, 14.0, 18.0, 22.0, 26.0, 30.0].iter().enumerate() {
            let z_def = ctx.field.grid.sample_top_smooth(*x, 0.0).unwrap_or(21.0);
            planar.push_str(&format!(
                "G1 X{x:.1} Y0 Z{z_def:.3} E{}.{i}\n",
                i + 1
            ));
        }
        let (mapped, report) = apply_post_pass(
            &planar,
            &planar,
            &ctx,
            crate::MachineLimits {
                z_max_mm: 256.0,
                max_feed_mm_per_min: 6000.0,
            },
        )
        .expect("post pass");
        let extrusion_zs: Vec<f64> = mapped
            .lines()
            .filter(|l| l.starts_with("G1") && l.contains('E') && l.contains('Z'))
            .filter_map(|l| {
                let i = l.find('Z')? + 1;
                l[i..].split_whitespace().next()?.parse().ok()
            })
            .collect();
        let z_spread = extrusion_zs
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
            - extrusion_zs
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min);
        assert!(
            z_spread > 0.12,
            "mapped underside extrusions should vary in Z (spread {z_spread:.3} mm)\n{mapped}"
        );
        println!("MINMAX {:?} {:?}\nMAPPED\n{mapped}", surface.min, surface.max);
        let auto = report.auto_placement.expect("report");
        let band = auto
            .surfaces
            .into_iter()
            .find(|s| s.id == surface.id)
            .expect("surface in report");
        let measured_vert = band
            .overhang_angle_measured_after_deg
            .expect("measured angle from vertical");
        let measured_slope = band
            .overhang_slope_measured_from_horizontal_deg
            .expect("measured slope from horizontal");
        use crate::pipeline::max_extrusion_slope_in_box;
        let slope = max_underside_band_slope_deg(
            &mapped,
            surface.min,
            surface.max,
            surface.bearing_deg,
            8,
        )
        .or_else(|| max_extrusion_slope_in_box(&mapped, surface.min, surface.max))
        .unwrap_or(0.0);
        assert!(
            (slope - measured_slope).abs() < 0.05,
            "reported slope {measured_slope:.2}° should match scanner {slope:.2}°"
        );
        assert!(
            slope >= 2.0 && slope <= opts.max_angle_deg + 0.6,
            "mapped gcode slope should be 2–{:.1}° (saw {slope:.2}°)",
            opts.max_angle_deg + 0.6
        );
        assert!(
            (90.0 - measured_slope - measured_vert).abs() < 0.05,
            "vertical {measured_vert:.1}° must equal 90° − slope {measured_slope:.1}°"
        );
        assert!(
            measured_vert < band.overhang_angle_before_deg.unwrap() - 3.0,
            "measured overhang {measured_vert:.1}° should be below before {:.1}°",
            band.overhang_angle_before_deg.unwrap()
        );
        use crate::safety::max_curved_band_extrusion_slope_deg;
        let global_cap = max_curved_band_extrusion_slope_deg(&mapped, &ctx.field)
            .unwrap_or(0.0);
        assert!(
            global_cap <= opts.max_angle_deg + 0.6,
            "curved-band safety scanner should match PR #18 cap ({global_cap:.2}° > {:.1}°)",
            opts.max_angle_deg + 0.6
        );
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