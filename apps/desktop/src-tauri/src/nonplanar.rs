//! Non-planar settings on the desktop command path.
//!
//! The window does not grow its own slicer. [`prepare_slice`] checks the same
//! angle cap and printer envelope the MCP tools check, then `slice_model`
//! hands the options to `shale_core::slice`, which is the CLI's pipeline.
//! [`shape_report`] is the plain-language view the panel draws afterwards.

use serde::Serialize;
use shale_core::{
    check_user_max_angle, count_curved_extrusions, NonplanarOptions, NonplanarReport,
    PlannedSurfaceKind, UserSettings,
};

/// Shown beside every non-planar control. The envelopes are not measured data.
pub const ENVELOPE_NOTE: &str =
    "Head-clearance envelopes are conservative estimates. They have not been measured on a printer yet.";

/// Refuse a curved job the pipeline cannot honour. Planar (`off`) always passes.
pub fn prepare_slice(settings: &UserSettings, printer_id: &str) -> Result<(), String> {
    if !settings.nonplanar.mode.active() {
        return Ok(());
    }
    check_user_max_angle(settings.nonplanar.max_angle_deg)?;
    settings.nonplanar.check_printer(printer_id).map_err(|err| {
        format!(
            "{err}. Choose Off, or a Bambu printer that ships a head envelope."
        )
    })
}

pub fn printer_supports(printer_id: &str) -> bool {
    NonplanarOptions::allowed_for_printer(printer_id)
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NonplanarSurfaceView {
    pub id: u32,
    pub kind: String,
    pub enabled: bool,
    pub reason: String,
    pub tilt_deg: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct NonplanarView {
    pub mode: String,
    pub max_angle_deg: f64,
    pub strict: bool,
    pub underside_bands: bool,
    /// `curved` when the written file is the curved pass, `planar` when it fell back.
    pub outcome: String,
    pub fallback_to_planar: bool,
    pub fallback_reasons: Vec<String>,
    pub curved_move_count: usize,
    pub worst_slope_deg: Option<f64>,
    pub surfaces: Vec<NonplanarSurfaceView>,
    pub envelope_note: String,
}

/// The panel's report. Fallback reasons are the planner's own sentences.
/// Curved moves are counted on the G-code that was written, with the same
/// 0.05° rule as the CLI notes.
pub fn shape_report(
    report: &NonplanarReport,
    gcode: &str,
    options: &NonplanarOptions,
) -> NonplanarView {
    let curved = !report.fallback_to_planar;
    let worst = report
        .safety
        .as_ref()
        .and_then(|safety| safety.max_extrusion_slope_deg)
        .filter(|slope| slope.is_finite());
    let surfaces = report
        .auto_placement
        .as_ref()
        .map(|plan| {
            plan.surfaces
                .iter()
                .map(|surface| NonplanarSurfaceView {
                    id: surface.id,
                    kind: kind_label(surface.kind).to_string(),
                    enabled: surface.enabled,
                    reason: surface.reason.clone(),
                    tilt_deg: surface.tilt_deg,
                })
                .collect()
        })
        .unwrap_or_default();
    NonplanarView {
        mode: if report.mode.is_empty() {
            options.mode.name().to_string()
        } else {
            report.mode.clone()
        },
        max_angle_deg: report.max_angle_deg,
        strict: options.strict,
        underside_bands: options.underside_bands,
        outcome: if curved {
            "curved".to_string()
        } else {
            "planar".to_string()
        },
        fallback_to_planar: report.fallback_to_planar,
        fallback_reasons: report.fallback_reasons.clone(),
        curved_move_count: if curved {
            count_curved_extrusions(gcode)
        } else {
            0
        },
        worst_slope_deg: worst,
        surfaces,
        envelope_note: ENVELOPE_NOTE.to_string(),
    }
}

fn kind_label(kind: PlannedSurfaceKind) -> &'static str {
    match kind {
        PlannedSurfaceKind::CurvedTopBand => "Curved top",
        PlannedSurfaceKind::SlopedTopBand => "Sloped top",
        PlannedSurfaceKind::BridgeTiltBand => "Bridge",
        PlannedSurfaceKind::CantileverTiltBand => "Cantilever",
        PlannedSurfaceKind::OverhangTiltBand => "Overhang",
    }
}

/// Per-segment slope in degrees. Points are copied through unchanged: the
/// preview draws the G-code's own Z, including a curved rise inside one layer.
pub fn segment_slopes(points: &[[f32; 3]]) -> Vec<f32> {
    if points.len() < 2 {
        return Vec::new();
    }
    points
        .windows(2)
        .map(|pair| {
            let (from, to) = (pair[0], pair[1]);
            shale_core::segment_slope_deg(
                f64::from(to[0] - from[0]),
                f64::from(to[1] - from[1]),
                f64::from(to[2] - from[2]),
            ) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shale_core::{
        NonplanarMode, NonplanarOptions, SafetyReport, CURVED_EXTRUSION_MIN_SLOPE_DEG,
    };

    fn options(mode: NonplanarMode, angle: f64) -> UserSettings {
        UserSettings {
            nonplanar: NonplanarOptions {
                mode,
                max_angle_deg: angle,
                ..NonplanarOptions::default()
            },
            ..UserSettings::default()
        }
    }

    #[test]
    fn off_skips_the_angle_cap_and_the_printer_check() {
        let mut nonplanar = options(NonplanarMode::Off, 45.0).nonplanar;
        nonplanar.strict = true;
        let settings = UserSettings {
            nonplanar,
            ..UserSettings::default()
        };
        prepare_slice(&settings, "creality_k1").expect("planar is always allowed");
    }

    #[test]
    fn auto_accepts_ten_degrees_on_the_a1_and_refuses_past_the_cap() {
        prepare_slice(&options(NonplanarMode::Auto, 10.0), "bambu_lab_a1").expect("cap");
        prepare_slice(&options(NonplanarMode::CurvedTops, 0.5), "bambu_lab_a1").expect("gentle");
        let high = prepare_slice(&options(NonplanarMode::Auto, 10.01), "bambu_lab_a1");
        assert!(high.unwrap_err().contains("at most 10"));
        let zero = prepare_slice(&options(NonplanarMode::Auto, 0.0), "bambu_lab_x1_carbon");
        assert!(zero.unwrap_err().contains("greater than 0"));
    }

    #[test]
    fn auto_names_the_way_out_when_the_printer_has_no_envelope() {
        let err = prepare_slice(&options(NonplanarMode::Auto, 10.0), "creality_k1").unwrap_err();
        assert!(err.contains("creality_k1"), "{err}");
        assert!(err.contains("Choose Off"), "{err}");
        assert!(!printer_supports("creality_k1"));
        assert!(printer_supports("bambu_lab_a1"));
    }

    #[test]
    fn the_report_keeps_every_fallback_reason_and_the_surface_list() {
        let report = NonplanarReport {
            mode: "auto".into(),
            max_angle_deg: 10.0,
            fallback_to_planar: true,
            fallback_reasons: vec![
                "no printable curved top surface".into(),
                "16 travel move(s) collide with printed material (tolerance 0.1 mm)".into(),
            ],
            auto_placement: Some(shale_core::AutoPlacementReport {
            mode: "auto".into(),
            max_angle_deg: 10.0,
            detection: shale_core::DetectionReport { features: Vec::new() },
            surfaces: vec![shale_core::PlannedSurface {
                id: 1,
                feature_id: 4,
                kind: PlannedSurfaceKind::CurvedTopBand,
                enabled: true,
                min: [0.0; 3],
                max: [1.0; 3],
                tilt_deg: 6.0,
                bearing_deg: 0.0,
                extent_mm: [10.0, 10.0],
                overhang_angle_before_deg: None,
                overhang_angle_after_deg: None,
                overhang_angle_measured_after_deg: None,
                overhang_slope_measured_from_horizontal_deg: None,
                reason: "curved-top band tilt 6.0°".into(),
            }],
            preview: Default::default(),
            enabled_surface_count: 1,
            fallback_to_planar: false,
            notes: Vec::new(),
            }),
            safety: Some(SafetyReport {
                max_extrusion_slope_deg: Some(6.06),
                ..SafetyReport::default()
            }),
            ..NonplanarReport::default()
        };
        let gcode = "G1 X0 Y0 Z1 E0\nG1 X10 Y0 Z1.1 E1\n";
        let view = shape_report(&report, gcode, &options(NonplanarMode::Auto, 10.0).nonplanar);
        assert_eq!(view.outcome, "planar");
        assert_eq!(view.curved_move_count, 0, "a fallback file is not counted as curved");
        assert_eq!(view.fallback_reasons.len(), 2);
        assert!(view.fallback_reasons[0].contains("no printable curved top"));
        assert_eq!(view.surfaces.len(), 1);
        assert_eq!(view.surfaces[0].kind, "Curved top");
        assert!(view.surfaces[0].enabled);
        assert!((view.worst_slope_deg.unwrap() - 6.06).abs() < 1e-9);
        assert!(view.envelope_note.contains("conservative estimates"));
        assert!(view.envelope_note.contains("not been measured"));
    }

    #[test]
    fn a_curved_file_counts_moves_and_the_preview_keeps_true_z() {
        let report = NonplanarReport {
            mode: "auto".into(),
            max_angle_deg: 10.0,
            fallback_to_planar: false,
            safety: Some(SafetyReport {
                max_extrusion_slope_deg: Some(6.06),
                ..SafetyReport::default()
            }),
            ..NonplanarReport::default()
        };
        let gcode = "G1 X0 Y0 Z5 E0\nG1 X10 Y0 Z5.2 E1\nG1 X20 Y0 Z5.2 E2\n";
        let view = shape_report(&report, gcode, &options(NonplanarMode::Auto, 10.0).nonplanar);
        assert_eq!(view.outcome, "curved");
        assert_eq!(view.curved_move_count, 1);
        let points = [[0.0f32, 0.0, 5.0], [10.0, 0.0, 5.2], [20.0, 0.0, 5.2]];
        let slopes = segment_slopes(&points);
        assert_eq!(slopes.len(), 2);
        assert!(
            f64::from(slopes[0]) > CURVED_EXTRUSION_MIN_SLOPE_DEG,
            "the rise is a curved segment"
        );
        assert!(
            f64::from(slopes[1]) <= CURVED_EXTRUSION_MIN_SLOPE_DEG,
            "the flat run stays with its feature colour"
        );
        assert_eq!(points[1][2], 5.2, "slope colouring must not flatten Z");
    }

    /// Same lattice as `nonplanar_curved_lid`: a drop of more than 0.1 mm into an
    /// XY cell that already holds taller plastic. A sheared contour rises and
    /// falls, so a raw descending-Z count would flag a legitimate curve.
    fn z_inversions(gcode: &str) -> usize {
        use std::collections::HashMap;
        const CELL_MM: f32 = 0.4;
        const SAMPLE_MM: f32 = 0.25;
        const INVERSION_DROP_MM: f32 = 0.1;
        let toolpath = shale_core::gcode::parse_toolpaths(gcode);
        let mut printed: HashMap<(i32, i32), f32> = HashMap::new();
        let mut hits = 0usize;
        let cell = |x: f32, y: f32| ((x / CELL_MM).floor() as i32, (y / CELL_MM).floor() as i32);
        for layer in &toolpath.layers {
            for poly in &layer.polylines {
                if poly.kind == "travel" || poly.points.len() < 2 {
                    continue;
                }
                for pair in poly.points.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let len = (dx * dx + dy * dy).sqrt();
                    let steps = ((len / SAMPLE_MM).ceil() as usize).max(1);
                    for step in 0..=steps {
                        let t = step as f32 / steps as f32;
                        let x = a[0] + dx * t;
                        let y = a[1] + dy * t;
                        let z = a[2] + (b[2] - a[2]) * t;
                        let seen = printed.entry(cell(x, y)).or_insert(f32::NEG_INFINITY);
                        if z < *seen - INVERSION_DROP_MM {
                            hits += 1;
                        } else if z > *seen {
                            *seen = z;
                        }
                    }
                }
            }
        }
        hits
    }

    /// The same three calls `slice_model` makes: check, `shale_core::slice`, shape.
    /// Skipped when CuraEngine is not configured, the way the core lid tests are.
    #[test]
    fn slice_model_pipeline_curves_the_demo_lid_on_a1() {
        if std::env::var_os("SHALE_CURAENGINE").is_none()
            || std::env::var_os("SHALE_CURA_DIR").is_none()
            || std::env::var_os("SHALE_MATERIALS_DIR").is_none()
            || std::env::var_os("SHALE_ORCA_DIR").is_none()
        {
            eprintln!("skipping slice_model_pipeline_curves_the_demo_lid_on_a1: engine not configured");
            return;
        }
        use shale_core::engine::CliEngine;
        use shale_core::job::{self, PlacedModel};
        use shale_core::mesh::{self, TriangleMesh};
        use shale_core::{Catalog, ResourcePaths};

        let stl = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../examples/nonplanar-demo.stl");
        let mesh: TriangleMesh = mesh::load_mesh(&stl).expect("demo lid");
        let transform = mesh::place_on_bed(&mesh, 256.0, 256.0, false);
        let placed = vec![PlacedModel {
            name: "nonplanar-demo".into(),
            mesh,
            transform,
            settings: vec![("ironing_enabled".into(), "false".into())],
            slot: None,
        }];
        let settings = UserSettings {
            layer_height: Some(0.2),
            ..options(NonplanarMode::Auto, 10.0)
        };
        prepare_slice(&settings, "bambu_lab_a1").expect("A1 auto at 10°");
        let catalog = Catalog::open(ResourcePaths::discover().expect("resources")).expect("catalog");
        let engine = CliEngine::from_env();
        assert!(engine.available(), "CuraEngine probe");
        let path = std::env::temp_dir().join("shale").join(format!(
            "desktop-nonplanar-a1-{}.gcode",
            std::process::id()
        ));
        let outcome = job::slice(
            &catalog,
            &engine,
            "bambu_lab_a1",
            Some("0.4 mm".into()),
            Some("0_20mm_standard_bbl_a1".into()),
            &[Some("bambu_lab_pla_basic".into())],
            settings.clone(),
            &placed,
            &[],
            None,
            &[],
            path.clone(),
            &mut |_| {},
        )
        .expect("slice through the core path slice_model uses");
        let gcode = std::fs::read_to_string(&path).expect("gcode");
        let report = outcome.nonplanar_report.expect("report");
        let view = shape_report(&report, &gcode, &settings.nonplanar);
        assert_eq!(view.outcome, "curved", "fell back: {:?}", view.fallback_reasons);
        assert!(
            (120_000..=150_000).contains(&view.curved_move_count),
            "curved moves {}, expected around 134862",
            view.curved_move_count
        );
        let slope = view.worst_slope_deg.expect("slope");
        assert!(
            (5.5..=7.0).contains(&slope),
            "worst slope {slope:.2}°, expected around 6.06"
        );
        let safety = report.safety.expect("safety");
        assert_eq!(safety.collisions_extruding, 0);
        assert_eq!(safety.collisions_travel, 0);
        assert_eq!(
            z_inversions(&gcode),
            0,
            "the nozzle must not drop back into material it has already printed"
        );
        assert!(view.surfaces.iter().any(|surface| surface.enabled));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn desktop_options_round_trip_the_four_controls() {
        let raw = r#"{"mode":"curved-tops","max_angle_deg":7.5,"strict":true,"underside_bands":false}"#;
        let parsed: NonplanarOptions = serde_json::from_str(raw).expect("controls");
        assert_eq!(parsed.mode, NonplanarMode::CurvedTops);
        assert_eq!(parsed.max_angle_deg, 7.5);
        assert!(parsed.strict);
        assert!(!parsed.underside_bands);
        assert_eq!(parsed.grid_step_mm, 0.35);
        let settings = UserSettings {
            nonplanar: parsed,
            ..UserSettings::default()
        };
        prepare_slice(&settings, "bambu_lab_p1s").expect("curved tops on a Bambu machine");
    }
}
