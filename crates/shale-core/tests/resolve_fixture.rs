use std::path::PathBuf;

use shale_core::{Catalog, ResourcePaths, UserSettings};

fn catalog() -> Catalog {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    Catalog::open(ResourcePaths {
        cura: root.join("cura"),
        materials: root.join("materials"),
    })
    .expect("fixture catalog")
}

#[test]
fn inheritance_formulas_variant_and_user_overrides() {
    let catalog = catalog();
    let printers = catalog.printers();
    assert!(printers.iter().any(|printer| printer.id == "demo_ender"));
    assert!(printers.iter().all(|printer| printer.id != "demo_base"));

    let variants = catalog.variants_for("demo_ender");
    assert!(variants
        .iter()
        .any(|variant| variant.name == "0.6mm Nozzle"));

    let resolved = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "demo_ender".into(),
            variant_name: Some("0.6mm Nozzle".into()),
            quality_type: Some("draft".into()),
            material_id: Some("generic_pla".into()),
            intent_category: None,
            settings: UserSettings {
                layer_height: Some(0.16),
                infill_sparse_density: Some(25.0),
                support_enable: Some(true),
                material_print_temperature: Some(205.0),
                material_bed_temperature: Some(55.0),
                extra: Vec::new(),
            },
        })
        .unwrap();

    let global = &resolved.global;
    assert_eq!(global.get("machine_width").map(String::as_str), Some("235"));
    assert_eq!(global.get("machine_depth").map(String::as_str), Some("235"));
    assert_eq!(
        global.get("machine_height").map(String::as_str),
        Some("250")
    );
    assert_eq!(
        global.get("machine_name").map(String::as_str),
        Some("Demo Ender")
    );
    assert_eq!(global.get("layer_height").map(String::as_str), Some("0.16"));
    assert_eq!(global.get("line_width").map(String::as_str), Some("0.6"));
    assert_eq!(
        global.get("infill_sparse_density").map(String::as_str),
        Some("25")
    );
    assert_eq!(
        global.get("infill_line_distance").map(String::as_str),
        Some("2.4")
    );
    assert_eq!(
        global.get("infill_pattern").map(String::as_str),
        Some("cubic")
    );
    assert_eq!(
        global.get("support_enable").map(String::as_str),
        Some("true")
    );
    assert_eq!(
        global.get("material_print_temperature").map(String::as_str),
        Some("205")
    );
    assert_eq!(
        global.get("material_bed_temperature").map(String::as_str),
        Some("55")
    );
    assert_eq!(
        global.get("adhesion_type").map(String::as_str),
        Some("skirt")
    );
    assert_eq!(global.get("speed_print").map(String::as_str), Some("50"));
    assert_eq!(global.get("speed_travel").map(String::as_str), Some("150"));
    assert_eq!(global.get("support_angle").map(String::as_str), Some("61"));
    assert_eq!(global.get("broken_setting").map(String::as_str), Some("1"));
    assert!(global
        .get("machine_start_gcode")
        .is_some_and(|gcode| gcode.contains("G28")));
    assert_eq!(
        global.get("machine_end_gcode").map(String::as_str),
        Some("G1 X0 Y235\\n")
    );
    assert!(resolved
        .warnings
        .iter()
        .any(|warning| warning.contains("broken_setting")));
    assert!((resolved.summary.nozzle_size - 0.6).abs() < 1e-6);
    assert!(resolved.summary.support_enable);
}

#[test]
fn real_cura_profiles_when_resources_are_present() {
    let Ok(cura) = std::env::var("SHALE_CURA_DIR") else {
        return;
    };
    let Ok(materials) = std::env::var("SHALE_MATERIALS_DIR") else {
        return;
    };
    let catalog = Catalog::open(ResourcePaths {
        cura: cura.into(),
        materials: materials.into(),
    })
    .expect("cura catalog");
    assert!(catalog
        .printers()
        .iter()
        .any(|printer| printer.id == "creality_ender3"));
    assert!(catalog
        .printers()
        .iter()
        .any(|printer| printer.id == "prusa_i3"));

    let resolved = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "creality_ender3".into(),
            variant_name: Some("0.4mm Nozzle".into()),
            quality_type: Some("draft".into()),
            material_id: Some("generic_pla".into()),
            intent_category: None,
            settings: UserSettings {
                layer_height: Some(0.2),
                infill_sparse_density: Some(20.0),
                support_enable: Some(false),
                material_print_temperature: Some(200.0),
                material_bed_temperature: Some(60.0),
                extra: Vec::new(),
            },
        })
        .expect("resolve ender-3");

    eprintln!(
        "ender-3 warnings {} (showing {})",
        resolved.warning_count,
        resolved.warnings.len()
    );
    for warning in &resolved.warnings {
        eprintln!("  {warning}");
    }
    eprintln!(
        "summary width={} line={} layer={} infill_dist={} adhesion={} speed={} nozzle={} bed={} diameter={} material={} pattern={}",
        resolved.summary.machine_width,
        resolved.summary.line_width,
        resolved.summary.layer_height,
        resolved.summary.infill_line_distance,
        resolved.summary.adhesion_type,
        resolved.summary.speed_print,
        resolved.summary.nozzle_temperature,
        resolved.summary.bed_temperature,
        resolved.summary.material_diameter,
        resolved.material_id,
        resolved.global.get("infill_pattern").map(String::as_str).unwrap_or("")
    );

    assert!((resolved.summary.machine_width - 235.0).abs() < 0.1);
    assert!((resolved.summary.machine_depth - 235.0).abs() < 0.1);
    assert!((resolved.summary.machine_height - 250.0).abs() < 0.1);
    assert!((resolved.summary.layer_height - 0.2).abs() < 1e-6);
    assert!((resolved.summary.line_width - 0.4).abs() < 0.05);
    assert!(resolved.summary.infill_line_distance > 0.5);
    assert!(!resolved.summary.support_enable);
    assert!((resolved.summary.nozzle_temperature - 200.0).abs() < 0.1);
    assert!((resolved.summary.bed_temperature - 60.0).abs() < 0.1);
    assert!(resolved
        .global
        .get("machine_start_gcode")
        .is_some_and(|gcode| gcode.contains("G28")));
    for key in [
        "layer_height",
        "line_width",
        "machine_width",
        "infill_line_distance",
        "adhesion_type",
        "speed_print",
        "material_print_temperature",
    ] {
        assert!(
            !resolved
                .warnings
                .iter()
                .any(|warning| warning.starts_with(&format!("{key}:"))),
            "{key} fell back: {:?}",
            resolved.warnings
        );
    }

    let prusa = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "prusa_i3".into(),
            variant_name: None,
            quality_type: Some("normal".into()),
            material_id: Some("generic_pla".into()),
            intent_category: None,
            settings: UserSettings {
                layer_height: Some(0.2),
                infill_sparse_density: Some(15.0),
                support_enable: Some(true),
                material_print_temperature: Some(210.0),
                material_bed_temperature: Some(60.0),
                extra: Vec::new(),
            },
        })
        .expect("resolve prusa");
    assert!((prusa.summary.machine_width - 200.0).abs() < 0.1);
    assert!(!prusa.summary.machine_center_is_zero);
    assert!(prusa.summary.support_enable);
    assert!(prusa
        .global
        .get("machine_start_gcode")
        .is_some_and(|gcode| gcode.contains("G28")));
}

#[test]
fn slice_cube_when_engine_present() {
    let Ok(program) = std::env::var("SHALE_CURAENGINE") else {
        return;
    };
    let Ok(cura) = std::env::var("SHALE_CURA_DIR") else {
        return;
    };
    let Ok(materials) = std::env::var("SHALE_MATERIALS_DIR") else {
        return;
    };
    let catalog = Catalog::open(ResourcePaths {
        cura: cura.into(),
        materials: materials.into(),
    })
    .unwrap();
    let dir = std::env::temp_dir().join("shale-slice-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("cube.stl");
    std::fs::write(
        &stl,
        "solid cube
facet normal 0 0 -1
 outer loop
  vertex 0 0 0
  vertex 20 0 0
  vertex 20 20 0
 endloop
endfacet
facet normal 0 0 -1
 outer loop
  vertex 0 0 0
  vertex 20 20 0
  vertex 0 20 0
 endloop
endfacet
facet normal 0 0 1
 outer loop
  vertex 0 0 10
  vertex 20 20 10
  vertex 20 0 10
 endloop
endfacet
facet normal 0 0 1
 outer loop
  vertex 0 0 10
  vertex 0 20 10
  vertex 20 20 10
 endloop
endfacet
facet normal 0 -1 0
 outer loop
  vertex 0 0 0
  vertex 20 0 10
  vertex 20 0 0
 endloop
endfacet
facet normal 0 -1 0
 outer loop
  vertex 0 0 0
  vertex 0 0 10
  vertex 20 0 10
 endloop
endfacet
facet normal 0 1 0
 outer loop
  vertex 0 20 0
  vertex 20 20 0
  vertex 20 20 10
 endloop
endfacet
facet normal 0 1 0
 outer loop
  vertex 0 20 0
  vertex 20 20 10
  vertex 0 20 10
 endloop
endfacet
facet normal -1 0 0
 outer loop
  vertex 0 0 0
  vertex 0 20 0
  vertex 0 20 10
 endloop
endfacet
facet normal -1 0 0
 outer loop
  vertex 0 0 0
  vertex 0 20 10
  vertex 0 0 10
 endloop
endfacet
facet normal 1 0 0
 outer loop
  vertex 20 0 0
  vertex 20 0 10
  vertex 20 20 10
 endloop
endfacet
facet normal 1 0 0
 outer loop
  vertex 20 0 0
  vertex 20 20 10
  vertex 20 20 0
 endloop
endfacet
endsolid cube
",
    )
    .unwrap();
    let mesh = shale_core::load_mesh(&stl).unwrap();
    let engine = shale_core::CliEngine {
        program: program.into(),
        lib_dir: std::env::var("SHALE_CURAENGINE_LIB_DIR")
            .ok()
            .map(Into::into),
    };
    let outcome = shale_core::slice(
        &catalog,
        &engine,
        "creality_ender3",
        Some("0.4mm Nozzle".into()),
        Some("draft".into()),
        Some("generic_pla".into()),
        UserSettings {
            layer_height: Some(0.2),
            infill_sparse_density: Some(20.0),
            support_enable: Some(false),
            material_print_temperature: Some(200.0),
            material_bed_temperature: Some(60.0),
            extra: Vec::new(),
        },
        &[shale_core::PlacedModel {
            name: "cube".into(),
            mesh,
            transform: shale_core::Transform::default(),
            settings: Vec::new(),
        }],
        &[shale_core::CustomInstruction::Pause {
            layer: Some(10),
            height_mm: None,
            filament_change: true,
            message: Some("swap".into()),
        }],
        None,
        dir.join("cube.gcode"),
        &mut |progress| eprintln!("{:.0}% {}", progress.fraction * 100.0, progress.message),
    );
    match &outcome {
        Ok(outcome) => {
            let gcode = std::fs::read_to_string(&outcome.gcode_path).unwrap();
            assert!(
                gcode.contains(";LAYER:"),
                "gcode had no layers:\n{}",
                outcome.log_tail
            );
            assert!(gcode.contains("G1") || gcode.contains("G0"));
            assert!(
                gcode.contains("M600 ; swap"),
                "custom pause was not compiled into the engine G-code"
            );
            assert!(!outcome.toolpath.layers.is_empty(), "{}", outcome.log_tail);
            eprintln!(
                "sliced {} layers, print time {:?}",
                outcome.toolpath.layers.len(),
                outcome.print_time_seconds
            );
        }
        Err(err) => panic!("{err}"),
    }
}

#[test]
fn arcus_slices_the_cube_when_configured() {
    let Ok(program) = std::env::var("SHALE_CURAENGINE") else {
        return;
    };
    let Ok(cura) = std::env::var("SHALE_CURA_DIR") else {
        return;
    };
    let Ok(materials) = std::env::var("SHALE_MATERIALS_DIR") else {
        return;
    };
    let catalog = Catalog::open(ResourcePaths {
        cura: cura.into(),
        materials: materials.into(),
    })
    .expect("catalog");
    let cli = shale_core::CliEngine {
        program: program.into(),
        lib_dir: std::env::var("SHALE_CURAENGINE_LIB_DIR")
            .ok()
            .map(Into::into),
    };
    let engine = shale_core::engine::ArcusEngine::new(&cli);
    let mesh = shale_core::load_mesh(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/cube.stl"
    )))
    .expect("cube");
    let mut peak = 0.0f32;
    let mut layers = 0usize;
    let outcome = shale_core::slice(
        &catalog,
        &engine,
        "creality_ender3",
        Some("0.4mm Nozzle".into()),
        Some("draft".into()),
        Some("generic_pla".into()),
        UserSettings {
            layer_height: Some(0.2),
            infill_sparse_density: Some(15.0),
            support_enable: Some(false),
            material_print_temperature: Some(200.0),
            material_bed_temperature: Some(60.0),
            extra: Vec::new(),
        },
        &[shale_core::PlacedModel {
            name: "cube".into(),
            mesh,
            transform: shale_core::Transform::default(),
            settings: Vec::new(),
        }],
        &[],
        None,
        std::env::temp_dir().join("shale-arcus-cube.gcode"),
        &mut |progress| {
            peak = peak.max(progress.fraction);
            if progress.layer.is_some() {
                layers += 1;
            }
            eprintln!("{:.0}% {}", progress.fraction * 100.0, progress.message);
        },
    );
    match outcome {
        Ok(outcome) => {
            let gcode = std::fs::read_to_string(&outcome.gcode_path).unwrap();
            assert!(gcode.contains(";LAYER:"), "{}", outcome.log_tail);
            assert!(peak > 0.2, "progress peaked at {peak}");
            assert!(layers > 0, "Arcus sent no layer previews");
            eprintln!(
                "arcus layers streamed {layers}, print time {:?}",
                outcome.print_time_seconds
            );
        }
        Err(err) => panic!("{err}"),
    }
}
