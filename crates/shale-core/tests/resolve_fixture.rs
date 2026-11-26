use std::path::PathBuf;
use std::sync::OnceLock;

use shale_core::{
    integration_test_cache_dir, set_catalog_cache_dir, Catalog, ResourcePaths, UserSettings,
};

static TEST_CACHE_READY: OnceLock<()> = OnceLock::new();
static REAL_CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();

fn use_test_cache_dir() {
    TEST_CACHE_READY.get_or_init(|| {
        set_catalog_cache_dir(integration_test_cache_dir("shale-core-resolve_fixture"));
    });
}

fn shared_real_catalog() -> Option<&'static Catalog> {
    use_test_cache_dir();
    REAL_CATALOG
        .get_or_init(|| real_resource_paths().and_then(|paths| Catalog::open(paths).ok()))
        .as_ref()
}

fn real_resource_paths() -> Option<ResourcePaths> {
    if let Ok(paths) = ResourcePaths::discover() {
        return Some(paths);
    }
    if let (Ok(cura), Ok(materials)) = (
        std::env::var("SHALE_CURA_DIR"),
        std::env::var("SHALE_MATERIALS_DIR"),
    ) {
        let paths = ResourcePaths {
            cura: cura.into(),
            materials: materials.into(),
        };
        if paths
            .resources()
            .join("definitions")
            .join("fdmprinter.def.json")
            .is_file()
        {
            return Some(paths);
        }
    }
    None
}

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
                ..UserSettings::default()
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
    let Some(catalog) = shared_real_catalog() else {
        eprintln!("skip real_cura_profiles_when_resources_are_present: no Cura resources (run scripts/fetch-resources.sh or set SHALE_CURA_DIR / SHALE_MATERIALS_DIR)");
        return;
    };
    assert!(catalog
        .printers()
        .iter()
        .any(|printer| printer.id == "creality_ender3"));
    assert!(catalog
        .printers()
        .iter()
        .any(|printer| printer.id == "prusa_i3"));
    let selectable = catalog.selectable_printers();
    assert!(
        selectable
            .iter()
            .any(|printer| printer.id == "bambu_lab_a1"),
        "Bambu A1 should be selectable"
    );
    assert!(
        !selectable
            .iter()
            .any(|printer| printer.id == "creality_ender3"),
        "Creality should stay in the catalogue but not be selectable"
    );
    assert!(
        selectable.len() < catalog.printers().len(),
        "selectable list should be smaller than the full catalogue"
    );

    let a1_04 = catalog.qualities_for("bambu_lab_a1", Some("0.4 mm"));
    assert!(
        !a1_04.is_empty(),
        "A1 0.4 mm process list must not be empty: {:?}",
        a1_04
    );
    assert!(
        a1_04.iter().any(|q| q.name == "0.20 mm Standard"),
        "missing 0.20 mm Standard; got {:?}",
        a1_04.iter().map(|q| q.name.as_str()).collect::<Vec<_>>()
    );
    assert!(
        a1_04.iter().any(|q| q.name.contains("0.28 mm")),
        "missing 0.28 mm Extra Draft class preset; got {:?}",
        a1_04
    );
    assert!(
        !a1_04
            .iter()
            .any(|q| q.quality_type.contains("_0_2_nozzle")
                || q.quality_type.contains("_0_6_nozzle")),
        "other-nozzle slugs leaked into 0.4 list: {:?}",
        a1_04
    );
    assert!(
        !a1_04
            .iter()
            .any(|q| q.name == "Normal" || q.name == "Draft"),
        "generic Cura tabs should not appear for Bambu A1"
    );

    let a1_02 = catalog.qualities_for("bambu_lab_a1", Some("0.2 mm"));
    assert!(
        a1_02.len() >= 4,
        "expected A1 0.2 mm presets, got {}: {:?}",
        a1_02.len(),
        a1_02
    );
    assert!(
        a1_02
            .iter()
            .all(|q| q.quality_type.ends_with("_0_2_nozzle")),
        "0.2 mm list should be nozzle-specific: {:?}",
        a1_02
    );
    assert!(
        a1_02.iter().any(|q| q.name.contains("0.12 mm")),
        "expected 0.12 mm class preset on 0.2 nozzle"
    );

    let material = catalog
        .materials_for_printer("bambu_lab_a1", 1.75)
        .into_iter()
        .find(|m| m.id.contains("pla"))
        .map(|m| m.id)
        .unwrap_or_else(|| "bambu_pla_basic".into());
    let extra_draft = a1_04
        .iter()
        .find(|q| q.name.starts_with("0.28 mm"))
        .expect("0.28 mm Extra Draft preset");
    let standard = a1_04
        .iter()
        .find(|q| q.name == "0.20 mm Standard")
        .expect("0.20 mm Standard preset");
    let resolved_extra = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "bambu_lab_a1".into(),
            variant_name: Some("0.4 mm".into()),
            quality_type: Some(extra_draft.quality_type.clone()),
            material_id: Some(material.clone()),
            intent_category: None,
            settings: UserSettings::default(),
        })
        .expect("resolve extra draft");
    let resolved_standard = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "bambu_lab_a1".into(),
            variant_name: Some("0.4 mm".into()),
            quality_type: Some(standard.quality_type.clone()),
            material_id: Some(material.clone()),
            intent_category: None,
            settings: UserSettings::default(),
        })
        .expect("resolve standard");
    assert!(
        (resolved_extra.summary.layer_height - 0.28).abs() < 0.02,
        "0.28 mm preset layer_height {}",
        resolved_extra.summary.layer_height
    );
    assert!(
        (resolved_standard.summary.layer_height - 0.20).abs() < 0.02,
        "standard layer_height {}",
        resolved_standard.summary.layer_height
    );
    let override_standard = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "bambu_lab_a1".into(),
            variant_name: Some("0.4 mm".into()),
            quality_type: Some(standard.quality_type.clone()),
            material_id: Some(material.clone()),
            intent_category: None,
            settings: UserSettings {
                layer_height: Some(0.16),
                ..UserSettings::default()
            },
        })
        .expect("resolve standard with layer_height override");
    assert_eq!(
        override_standard
            .global
            .get("layer_height")
            .map(String::as_str),
        Some("0.16"),
        "user layer_height should beat 0.20 mm Standard preset"
    );
    assert_eq!(
        resolved_standard
            .global
            .get("speed_wall_0")
            .map(String::as_str),
        Some("200"),
        "Bambu outer wall speed"
    );
    assert_eq!(
        resolved_standard
            .global
            .get("speed_wall_x")
            .map(String::as_str),
        Some("300"),
        "Bambu inner wall speed"
    );
    assert_eq!(
        resolved_standard
            .global
            .get("speed_infill")
            .map(String::as_str),
        Some("270"),
        "Bambu sparse infill speed"
    );

    let friendly = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "bambu_lab_a1".into(),
            variant_name: Some("0.4 mm".into()),
            quality_type: Some("standard".into()),
            material_id: Some(material.clone()),
            intent_category: None,
            settings: UserSettings::default(),
        })
        .expect("friendly quality standard");
    assert!(
        (friendly.summary.layer_height - 0.20).abs() < 0.02,
        "standard alias layer_height {}",
        friendly.summary.layer_height
    );
    assert_eq!(
        friendly.quality_type, standard.quality_type,
        "standard alias should map to {}",
        standard.quality_type
    );

    let by_label = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "bambu_lab_a1".into(),
            variant_name: Some("0.4 mm".into()),
            quality_type: Some("0.20mm Standard".into()),
            material_id: Some(material.clone()),
            intent_category: None,
            settings: UserSettings::default(),
        })
        .expect("0.20mm Standard label");
    assert_eq!(by_label.quality_type, standard.quality_type);

    let err = catalog
        .resolve(&shale_core::ResolveRequest {
            printer_id: "bambu_lab_a1".into(),
            variant_name: Some("0.4 mm".into()),
            quality_type: Some("definitely-not-a-preset".into()),
            material_id: Some(material.clone()),
            intent_category: None,
            settings: UserSettings::default(),
        })
        .unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("unknown quality"),
        "error should list valid choices: {message}"
    );
    assert!(
        message.contains("0.20 mm Standard"),
        "error should mention a real preset: {message}"
    );
    assert_eq!(
        resolved_standard
            .global
            .get("speed_travel")
            .map(String::as_str),
        Some("700"),
        "Bambu travel speed on 0.20 mm Standard @ A1"
    );
    assert!(
        resolved_standard
            .global
            .get("machine_max_acceleration_x")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
            >= 10_000.0,
        "A1 machine acceleration limits should flow through"
    );
    assert!(
        (resolved_extra.summary.layer_height - resolved_standard.summary.layer_height).abs() > 0.02,
        "preset should change layer_height"
    );

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
                ..UserSettings::default()
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
                ..UserSettings::default()
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

fn preset_layer_mm(name: &str) -> Option<f64> {
    let head = name.split_whitespace().next()?;
    let value = head.trim_end_matches("mm");
    value.parse().ok().filter(|layer| *layer < 1.0)
}

#[test]
fn a1_every_selectable_process_resolves_layer_height() {
    let Some(catalog) = shared_real_catalog() else {
        eprintln!("skip a1_every_selectable_process_resolves_layer_height: no Cura resources");
        return;
    };
    let material = catalog
        .materials_for_printer("bambu_lab_a1", 1.75)
        .into_iter()
        .find(|m| m.label.to_ascii_lowercase().contains("pla"))
        .map(|m| m.id)
        .expect("PLA material for A1");
    for variant in ["0.2 mm", "0.4 mm", "0.6 mm", "0.8 mm"] {
        let presets = catalog.qualities_for("bambu_lab_a1", Some(variant));
        assert!(!presets.is_empty(), "no presets for A1 at {variant}");
        for preset in presets {
            let want = preset_layer_mm(&preset.name).unwrap_or_else(|| {
                panic!("preset name lacks layer hint: {}", preset.name);
            });
            let resolved = catalog
                .resolve(&shale_core::ResolveRequest {
                    printer_id: "bambu_lab_a1".into(),
                    variant_name: Some(variant.into()),
                    quality_type: Some(preset.quality_type.clone()),
                    material_id: Some(material.clone()),
                    intent_category: None,
                    settings: UserSettings::default(),
                })
                .unwrap_or_else(|err| panic!("{} @ {}: {err}", preset.name, variant));
            assert!(
                (resolved.summary.layer_height - want).abs() < 0.021,
                "{} @ {}: expected layer_height {want}, summary {}, global {:?}",
                preset.name,
                variant,
                resolved.summary.layer_height,
                resolved.global.get("layer_height")
            );
            let global = resolved
                .global
                .get("layer_height")
                .and_then(|text| text.parse::<f64>().ok())
                .unwrap_or(resolved.summary.layer_height);
            assert!(
                (global - want).abs() < 0.021,
                "{} @ {}: engine global layer_height {global}, want {want}",
                preset.name,
                variant
            );
        }
    }
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
        &[Some("generic_pla".into())],
        UserSettings {
            layer_height: Some(0.2),
            infill_sparse_density: Some(20.0),
            support_enable: Some(false),
            material_print_temperature: Some(200.0),
            material_bed_temperature: Some(60.0),
            extra: Vec::new(),
            ..UserSettings::default()
        },
        &[shale_core::PlacedModel {
            name: "cube".into(),
            mesh,
            transform: shale_core::Transform::default(),
            settings: Vec::new(),
            slot: None,
        }],
        &[shale_core::CustomInstruction::Pause {
            layer: Some(10),
            height_mm: None,
            filament_change: true,
            message: Some("swap".into()),
        }],
        None,
        &[],
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
        &[Some("generic_pla".into())],
        UserSettings {
            layer_height: Some(0.2),
            infill_sparse_density: Some(15.0),
            support_enable: Some(false),
            material_print_temperature: Some(200.0),
            material_bed_temperature: Some(60.0),
            extra: Vec::new(),
            ..UserSettings::default()
        },
        &[shale_core::PlacedModel {
            name: "cube".into(),
            mesh,
            transform: shale_core::Transform::default(),
            settings: Vec::new(),
            slot: None,
        }],
        &[],
        None,
        &[],
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

/// Cura's own UI treats cool_fan_enabled as the switch over the fan and leaves
/// the speeds alone. The engine keeps emitting M106 at cool_fan_speed whatever
/// the switch says, so "part cooling off" has to carry the speeds down with it
/// or the switch changes nothing at all.
#[test]
fn turning_cooling_off_takes_the_fan_speeds_with_it() {
    let catalog = catalog();
    let resolve = |enabled: &str| {
        catalog
            .resolve(&shale_core::ResolveRequest {
                printer_id: "demo_ender".into(),
                variant_name: None,
                quality_type: None,
                material_id: Some("generic_pla".into()),
                intent_category: None,
                settings: UserSettings {
                    extra: vec![("cool_fan_enabled".to_string(), enabled.to_string())],
                    ..Default::default()
                },
            })
            .unwrap()
    };

    let on = resolve("True");
    let off = resolve("False");

    let speed = |resolved: &shale_core::ResolvedSettings| {
        resolved
            .global
            .get("cool_fan_speed")
            .and_then(|value| value.parse::<f64>().ok())
    };
    // The fixture profile carries no fan speed of its own; the engine would use
    // its built-in hundred percent. What matters is that the off switch writes a
    // speed of zero over whatever the profile said.
    assert!(!on.global.contains_key("cool_fan_speed"));
    assert_eq!(speed(&off), Some(0.0), "cooling off means the fan is off");
    assert_eq!(
        off.global.get("cool_fan_speed_max").map(String::as_str),
        Some("0")
    );
}
