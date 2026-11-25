//! `shale` — the same engine the window drives, without the window.
//!
//! No Tauri, no webview, no Node: this binary opens the Cura resource tree,
//! resolves a printer the way the app does, hands the mesh to an unmodified
//! CuraEngine, and writes a G-code file. It is meant to be copied to a machine
//! that has no desktop at all, so every path can be given by environment
//! variable and every failure names the path it tried.

mod info_env;

use std::path::PathBuf;
use std::process::ExitCode;

use info_env::{
    cura_engine_lib_path, cura_engine_path, cura_resources_path_from, env_path_line,
    materials_path_from, orca_profiles_path,
};

use shale_core::catalog::{Catalog, ResourcePaths};
use shale_core::engine::{CliEngine, EngineProgress};
use shale_core::job::{slice, PlacedModel};
use shale_core::mesh::{
    assign_plates, bed_overflow, load_build_objects, load_mesh, place_assembly, read_plate_groups,
    BuildObject, PlateGroup,
};
use shale_core::project_settings::{apply_project_settings, read_project_settings};
use shale_core::resolver::{ResolveRequest, UserSettings};
use shale_core::{NonplanarMode, NonplanarOptions};

const USAGE: &str = "\
shale — slice a model without the window

  shale info                     resources, engine and catalogue
  shale printers [--search TEXT]  printers the catalogue knows
  shale settings --printer ID [--variant NAME] [--quality TYPE] [--material ID]
  shale slice MODEL [options]    write G-code next to the model, or -o PATH
  shale suggest MODEL [options]  tips for the model, as text or --json
  shale projects                 recently opened or saved projects
  shale open PROJECT.shale       print a project and remember it
  shale save MODEL -o PROJECT.shale
                                 store a model as a project
  shale stats --gcode FILE       time and filament from a finished slice
  shale compare MODEL --patch KEY=VALUE [--patch KEY=VALUE]
                                 slice a few setting patches and compare them

slice options
  --printer ID        printer definition id (default: the first one found)
  --variant NAME      nozzle variant, e.g. \"0.4 mm\"
  --quality TYPE      quality profile type, e.g. \"standard\"
  --material ID       filament id, e.g. \"generic_pla\"
  --layer MM          layer height
  --infill PERCENT    sparse infill density
  --supports          enable support
  --set KEY=VALUE     any Cura setting, repeatable
  --nonplanar MODE    experimental: off, curved-tops, or auto (Bambu printers with head envelope)
  --nonplanar-strict  fail instead of falling back to a planar print when the curved pass cannot be verified
  --nonplanar-underside  let auto plan underside tilt bands too (unverified: the part is above them)
  --max-angle DEG     clearance limit for curved tops (default 10)
  --plate N           which plate of a multi-plate 3MF to slice (default 1)
  --all-plates        slice every plate; writes STEM-plate-N.gcode
  --ignore-project-settings
                      do not apply the 3MF's embedded process settings
  -o, --output PATH   where to write the G-code

suggest options
  --json              print the suggestions as JSON
  --printer ID        bed size and enclosure come from this printer when the catalogue is present
  --type TEXT         filament on slot 1, for example PLA or PETG
  --slot TEXT         another loaded spool, repeatable, slots 2, 3, ...
  --supports          turn supports on for the check
  --set KEY=VALUE     a plate setting, repeatable
  --gcode PATH        measure a finished slice (time sinks, purge)
  --dismiss ID        hide one suggestion id or rule id
  --humidity N        AMS humidity percent, only when the printer reported it
  --nonplanar MODE    off, auto, or curved-tops
  --multi-material    the printer has an AMS or a second extruder
  --tag TEXT          an explicit tag, for example hot-use

stats options
  --gcode PATH        the finished G-code file
  --price N           filament price per kilogram (default 20)
  --layers            print one duration per layer
  --json              print the stats as JSON

compare options
  --patch KEY=VALUE   one slice. Repeat the flag for another slice. Commas
                      separate extra keys: --patch infill_sparse_density=15,layer_height=0.2
  --concurrency N     how many slices run at once (1 to 3, default 2)
  --price N           filament price per kilogram
  plus the slice flags --printer, --variant, --quality, --material

environment
  SHALE_CURA_DIR, SHALE_MATERIALS_DIR   the Cura resource tree
  SHALE_ORCA_DIR                        Bambu/Orca profiles (optional; auto-detected)
  SHALE_CURAENGINE                      the CuraEngine binary
  SHALE_CURAENGINE_LIB_DIR              folder beside the engine with its libraries (optional)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("shale: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        print!("{USAGE}");
        return Ok(());
    };
    match command {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        "-V" | "--version" | "version" => {
            println!("shale {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "info" => info(),
        "printers" => printers(flag(args, "--search")),
        "settings" => settings(args),
        "slice" => slice_command(args),
        "suggest" => suggest_command(args),
        "projects" => projects_command(),
        "open" => open_command(args),
        "save" => save_command(args),
        "stats" => stats_command(args),
        "compare" => compare_command(args),
        other => Err(format!("unknown command `{other}`. Try `shale help`.")),
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        if args[index] == name {
            return args.get(index + 1).cloned();
        }
        index += 1;
    }
    None
}

fn many(args: &[String], name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == name {
            if let Some(value) = args.get(index + 1) {
                out.push(value.clone());
            }
        }
        index += 1;
    }
    out
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn catalog() -> Result<Catalog, String> {
    let paths = ResourcePaths::discover().map_err(|err| err.to_string())?;
    Catalog::open(paths).map_err(|err| err.to_string())
}

fn info() -> Result<(), String> {
    println!("shale {}", env!("CARGO_PKG_VERSION"));
    println!("{}", env_path_line("SHALE_CURAENGINE", cura_engine_path()));
    println!(
        "{}",
        env_path_line("SHALE_CURAENGINE_LIB_DIR", cura_engine_lib_path())
    );
    let discovered = ResourcePaths::discover();
    println!(
        "{}",
        env_path_line("SHALE_CURA_DIR", cura_resources_path_from(&discovered))
    );
    println!(
        "{}",
        env_path_line("SHALE_MATERIALS_DIR", materials_path_from(&discovered))
    );
    println!("{}", env_path_line("SHALE_ORCA_DIR", orca_profiles_path()));
    match discovered {
        Ok(paths) => match Catalog::open(paths) {
            Ok(catalog) => {
                let printers = catalog.selectable_printers();
                println!("printers {}", printers.len());
            }
            Err(err) => println!("catalog  unreadable: {err}"),
        },
        Err(err) => println!("catalog  not found — {err}"),
    }
    Ok(())
}

fn printers(search: Option<String>) -> Result<(), String> {
    let catalog = catalog()?;
    let needle = search.unwrap_or_default().to_lowercase();
    for printer in catalog.selectable_printers() {
        if !needle.is_empty()
            && !format!("{} {} {}", printer.id, printer.name, printer.manufacturer)
                .to_lowercase()
                .contains(&needle)
        {
            continue;
        }
        println!("{}\t{} {}", printer.id, printer.manufacturer, printer.name);
    }
    Ok(())
}

fn resolve_request(args: &[String], printer: &str) -> ResolveRequest {
    let mut extra = Vec::new();
    for entry in many(args, "--set") {
        if let Some((key, value)) = entry.split_once('=') {
            extra.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    ResolveRequest {
        printer_id: printer.to_string(),
        variant_name: flag(args, "--variant"),
        quality_type: flag(args, "--quality"),
        material_id: flag(args, "--material"),
        intent_category: None,
        settings: UserSettings {
            layer_height: flag(args, "--layer").and_then(|value| value.parse().ok()),
            infill_sparse_density: flag(args, "--infill").and_then(|value| value.parse().ok()),
            support_enable: if has(args, "--supports") {
                Some(true)
            } else {
                None
            },
            material_print_temperature: None,
            material_bed_temperature: None,
            extra,
            nonplanar: nonplanar_options(args),
        },
    }
}

fn nonplanar_options(args: &[String]) -> NonplanarOptions {
    let mode = flag(args, "--nonplanar")
        .and_then(|name| NonplanarMode::from_cli_name(&name))
        .unwrap_or(NonplanarMode::Off);
    let max_angle_deg = flag(args, "--max-angle")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10.0);
    NonplanarOptions {
        mode,
        max_angle_deg,
        strict: has(args, "--nonplanar-strict"),
        underside_bands: has(args, "--nonplanar-underside"),
        ..NonplanarOptions::default()
    }
}

fn default_printer(catalog: &Catalog) -> Result<String, String> {
    catalog
        .default_printer_id()
        .ok_or_else(|| {
            "the catalogue has no Bambu printers; point SHALE_CURA_DIR at a Cura resource tree with Bambu profiles"
                .to_string()
        })
}

fn settings(args: &[String]) -> Result<(), String> {
    let catalog = catalog()?;
    let printer = flag(args, "--printer").unwrap_or(default_printer(&catalog)?);
    let resolved = catalog
        .resolve(&resolve_request(args, &printer))
        .map_err(|err| err.to_string())?;
    println!(
        "# {} / {} / {} / {}",
        resolved.printer_name, resolved.variant_name, resolved.quality_type, resolved.material_id
    );
    for (key, value) in &resolved.global {
        println!("{key}={value}");
    }
    Ok(())
}

fn slice_command(args: &[String]) -> Result<(), String> {
    let model = args
        .iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .ok_or_else(|| "slice needs a model path. Try `shale help`.".to_string())?
        .clone();
    let model_path = PathBuf::from(&model);

    // Refuse a non-planar request on a printer with no toolhead envelope before
    // doing any work: an explicit `--printer` is checked here, the catalogue
    // default right after resolution.
    let nonplanar = nonplanar_options(args);
    if let Some(printer) = flag(args, "--printer") {
        nonplanar.check_printer(&printer)?;
    }

    let (groups, project) = load_plates(&model_path)?;
    let all_plates = has(args, "--all-plates");
    let plate_number = flag(args, "--plate").and_then(|value| value.parse::<u32>().ok());
    if all_plates && plate_number.is_some() {
        return Err("--plate and --all-plates cannot be used together".to_string());
    }
    let chosen: Vec<(PlateGroup, Vec<BuildObject>)> = if all_plates {
        groups.clone()
    } else {
        let number = plate_number.unwrap_or(1);
        let plate = groups
            .iter()
            .find(|(plate, _)| plate.number == number)
            .or_else(|| groups.get(number.saturating_sub(1) as usize))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "this file has no plate {number} (it has {})",
                    groups.len()
                )
            })?;
        vec![plate]
    };
    if !all_plates && groups.len() > 1 {
        let number = chosen[0].0.number;
        eprintln!(
            "shale: this file has {} plates; slicing plate {number}. Use --plate N or --all-plates for the others.",
            groups.len()
        );
    }

    let catalog = catalog()?;
    let printer = flag(args, "--printer").unwrap_or(default_printer(&catalog)?);
    nonplanar.check_printer(&printer)?;
    let mut request = resolve_request(args, &printer);
    if has(args, "--ignore-project-settings") {
        if project.is_some() {
            eprintln!("shale: embedded project settings ignored");
        }
    } else if let Some(project) = &project {
        apply_project_settings(&mut request.settings, project);
        eprintln!("shale: project settings");
        for setting in &project.summary.applied {
            eprintln!("  {} {}", setting.label.to_lowercase(), setting.detail);
        }
        eprintln!("  {}", project.summary.note);
    }
    let resolved = catalog.resolve(&request).map_err(|err| err.to_string())?;
    let width = resolved
        .global
        .get("machine_width")
        .and_then(|v| v.parse().ok())
        .unwrap_or(220.0);
    let depth = resolved
        .global
        .get("machine_depth")
        .and_then(|v| v.parse().ok())
        .unwrap_or(220.0);
    let height = resolved
        .global
        .get("machine_height")
        .and_then(|v| v.parse().ok())
        .unwrap_or(250.0);
    let centre = resolved.summary.machine_center_is_zero;

    let output = flag(args, "--output")
        .or_else(|| flag(args, "-o"))
        .map(PathBuf::from)
        .unwrap_or_else(|| model_path.with_extension("gcode"));
    let output = shale_core::engine::absolutize_path(&output).map_err(|err| err.to_string())?;

    let engine = CliEngine::from_env();
    engine.probe().map_err(|message| {
        format!(
            "CuraEngine is not usable at {}.\n{message}\nSet SHALE_CURAENGINE to the binary.",
            engine.program.display()
        )
    })?;

    for (plate, objects) in &chosen {
    let meshes: Vec<_> = objects.iter().map(|object| object.mesh.clone()).collect();
    let transform = place_assembly(&meshes, width, depth, centre);
    let mut overflow = [0.0_f64; 3];
    for mesh in &meshes {
        let part = bed_overflow(mesh, &transform, width, depth, height);
        for axis in 0..3 {
            overflow[axis] = overflow[axis].max(part[axis]);
        }
    }
    if overflow.iter().any(|millimetres| *millimetres > 0.5) {
        eprintln!(
            "shale: plate {number} extends outside the build volume (X {x:.1} mm, Y {y:.1} mm, Z {z:.1} mm past the plate).",
            number = plate.number,
            x = overflow[0],
            y = overflow[1],
            z = overflow[2]
        );
    }
    let placed: Vec<PlacedModel> = objects
        .iter()
        .map(|object| PlacedModel {
            name: if object.name.trim().is_empty() {
                format!("object {}", object.object_id)
            } else {
                object.name.clone()
            },
            mesh: object.mesh.clone(),
            transform: transform.clone(),
            settings: Vec::new(),
            slot: None,
        })
        .collect();
    let plate_output = if all_plates {
        let stem = output
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "plate".to_string());
        output.with_file_name(format!("{stem}-plate-{}.gcode", plate.number))
    } else {
        output.clone()
    };

    let outcome = slice(
        &catalog,
        &engine,
        &printer,
        request.variant_name.clone(),
        request.quality_type.clone(),
        std::slice::from_ref(&request.material_id),
        request.settings.clone(),
        &placed,
        &[],
        None,
        &[],
        plate_output.clone(),
        &mut |progress: EngineProgress| {
            if std::env::var_os("SHALE_TRACE").is_some() {
                eprintln!(
                    "[{}] {}",
                    (progress.fraction * 100.0) as u32,
                    progress.message
                );
            }
        },
    )
    .map_err(|err| err.to_string())?;

    println!("{}", outcome.gcode_path.display());
    if let Some(seconds) = outcome.print_time_seconds {
        println!(
            "time     {} h {} m",
            (seconds / 3600.0) as u32,
            ((seconds % 3600.0) / 60.0) as u32
        );
    }
    println!("filament {:.2} m", outcome.filament_metres);
    println!("layers   {}", outcome.toolpath.layers.len());
    if std::env::var_os("SHALE_BENCH_SUMMARY").is_some() {
        println!(
            "bench_summary {}",
            serde_json::json!({
                "print_time_seconds": outcome.print_time_seconds,
                "filament_m": outcome.filament_metres,
                "layers": outcome.toolpath.layers.len(),
            })
        );
    }
    if let Some(report) = &outcome.nonplanar_report {
        let collisions_extruding = report
            .safety
            .as_ref()
            .map_or(0, |safety| safety.collisions_extruding);
        let collisions_travel = report
            .safety
            .as_ref()
            .map_or(0, |safety| safety.collisions_travel);
        println!(
            "nonplanar {} islands={} flattened={} equiv={} fallback={} collisions_extruding={} collisions_travel={}",
            report.mode,
            report.top_islands,
            report.islands_flattened,
            report.planar_equivalence_ok,
            report.fallback_to_planar,
            collisions_extruding,
            collisions_travel
        );
        if report.fallback_to_planar {
            println!("fallback_reason {}", report.fallback_reasons.join(" | "));
        }
        if let Some(auto) = &report.auto_placement {
            // `auto.fallback_to_planar` is the planner's own view of the plan it
            // handed over; the authority on what was actually written is the
            // report. Printing the plan flag as `fallback_planar` made the CLI
            // announce `fallback_planar=false` directly under `fallback=true`.
            println!(
                "auto surfaces={} plan_fallback={} fallback_planar={}",
                auto.enabled_surface_count, auto.fallback_to_planar, report.fallback_to_planar
            );
            if std::env::var_os("SHALE_AUTO_PLAN_JSON").is_some() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(auto).unwrap_or_else(|_| "{}".into())
                );
            }
        }
        if let Some(safety) = &report.safety {
            println!(
                "nonplanar_safety ok={} violations={} max_slope={:.2}°",
                safety.ok,
                safety.violations.len(),
                safety.max_extrusion_slope_deg.unwrap_or(0.0)
            );
            if !safety.violations.is_empty() && std::env::var_os("SHALE_TRACE").is_some() {
                for v in &safety.violations {
                    eprintln!("  safety {}: {}", v.kind, v.detail);
                }
            }
        }
    }
    }
    Ok(())
}

type LoadedPlates = Vec<(PlateGroup, Vec<BuildObject>)>;

fn load_plates(
    path: &std::path::Path,
) -> Result<(LoadedPlates, Option<shale_core::ProjectSettings>), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model");
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension == "3mf" {
        let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
        let objects = load_build_objects(&bytes).map_err(|err| err.to_string())?;
        let grouped = assign_plates(objects, read_plate_groups(&bytes));
        if grouped.is_empty() {
            return Err("that file held no geometry".to_string());
        }
        return Ok((grouped, read_project_settings(&bytes)));
    }
    let mesh = load_mesh(path).map_err(|err| err.to_string())?;
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    Ok((
        vec![(
            PlateGroup {
                number: 1,
                name: String::new(),
                object_ids: vec![0],
            },
            vec![BuildObject {
                object_id: 0,
                name: stem,
                mesh,
            }],
        )],
        None,
    ))
}

fn suggest_command(args: &[String]) -> Result<(), String> {
    let model = args
        .iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-'))
        .ok_or("suggest needs a model path")?;
    let (plates, _) = load_plates(std::path::Path::new(model))?;
    let mut objects = Vec::new();
    for (plate_index, (_, builds)) in plates.iter().enumerate() {
        for build in builds {
            objects.push(shale_core::SceneObject {
                id: build.object_id.to_string(),
                name: build.name.clone(),
                mesh: build.mesh.clone(),
                transform: shale_core::Transform::default(),
                plate: plate_index,
                settings: std::collections::BTreeMap::new(),
                slot: None,
            });
        }
    }
    if objects.is_empty() {
        return Err("that file held no geometry".into());
    }
    let mut settings = std::collections::BTreeMap::new();
    if has(args, "--supports") {
        settings.insert("support_enable".into(), "1".into());
    }
    for entry in many(args, "--set") {
        if let Some((key, value)) = entry.split_once('=') {
            settings.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    let mut slots = Vec::new();
    let primary = flag(args, "--type").unwrap_or_else(|| "PLA".into());
    slots.push(shale_core::FilamentSlot {
        index: 0,
        material_id: primary.clone(),
        filament_type: primary,
        colour: String::new(),
        loaded: true,
    });
    for (offset, kind) in many(args, "--slot").into_iter().enumerate() {
        slots.push(shale_core::FilamentSlot {
            index: offset + 1,
            material_id: kind.clone(),
            filament_type: kind,
            colour: String::new(),
            loaded: true,
        });
    }
    let (bed_w, bed_d, bed_h, name, enclosed) = printer_bed(flag(args, "--printer").as_deref());
    let gcode = flag(args, "--gcode")
        .map(std::fs::read_to_string)
        .transpose()
        .map_err(|err| err.to_string())?
        .map(|text| shale_core::gcode_facts(&text));
    let scene = shale_core::Scene {
        objects,
        printer: shale_core::PrinterFacts {
            id: flag(args, "--printer").unwrap_or_else(|| "bambu_lab_a1".into()),
            name,
            bed_width_mm: bed_w,
            bed_depth_mm: bed_d,
            bed_height_mm: bed_h,
            nozzle_mm: 0.4,
            line_width_mm: 0.42,
            speed_mm_s: 200.0,
            bed_temperature_c: 60.0,
            enclosed,
            multi_material: has(args, "--multi-material") || slots.len() > 1,
        },
        slots,
        device_slots: None,
        humidity_percent: flag(args, "--humidity").and_then(|value| value.parse().ok()),
        hours_since_dry: None,
        settings,
        gcode,
        nonplanar_mode: flag(args, "--nonplanar").unwrap_or_else(|| "off".into()),
        tags: many(args, "--tag"),
        dismissed: many(args, "--dismiss"),
        global_dismissed: Vec::new(),
        shown_once: Vec::new(),
    };
    let found = shale_core::suggest::evaluate(&scene);
    if has(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&found).map_err(|err| err.to_string())?);
        return Ok(());
    }
    if found.is_empty() {
        println!("no suggestions");
        return Ok(());
    }
    for item in &found {
        let kind = match item.severity {
            shale_core::Severity::Tip => "tip",
            shale_core::Severity::Warning => "warning",
        };
        println!("{kind}\t{}\t{}", item.id, item.title);
        println!("\t{}", item.reason);
        if let Some(fix) = &item.fix {
            println!("\tapply\t{}", fix.label);
        }
    }
    Ok(())
}

fn printer_bed(id: Option<&str>) -> (f64, f64, f64, String, bool) {
    let fallback = (256.0, 256.0, 256.0, "Bambu Lab A1".to_string(), false);
    let Ok(catalog) = catalog() else {
        return fallback;
    };
    let id = match id {
        Some(id) => id.to_string(),
        None => match catalog.default_printer_id() {
            Some(id) => id,
            None => return fallback,
        },
    };
    // The id above may not be a real definition. Ask the catalogue and keep the
    // fallback bed when it is not.
    let request = shale_core::resolver::ResolveRequest {
        printer_id: id.to_string(),
        variant_name: None,
        quality_type: None,
        material_id: None,
        intent_category: None,
        settings: shale_core::resolver::UserSettings::default(),
    };
    match catalog.resolve(&request) {
        Ok(resolved) => (
            resolved.summary.machine_width,
            resolved.summary.machine_depth,
            resolved.summary.machine_height,
            resolved.summary.printer_name,
            shale_core::printer_is_enclosed(&resolved.printer_id),
        ),
        Err(_) => fallback,
    }
}

fn positional(args: &[String]) -> Option<String> {
    args.iter().skip(1).find(|arg| !arg.starts_with('-')).cloned()
}

fn projects_command() -> Result<(), String> {
    let entries = shale_core::load_recents(&shale_core::config_dir()).map_err(|err| err.to_string())?;
    if entries.is_empty() {
        println!("no recent projects");
        return Ok(());
    }
    for entry in entries {
        let missing = if entry.missing { " missing" } else { "" };
        println!("{}\t{}{missing}", entry.path, entry.name);
    }
    Ok(())
}

fn open_command(args: &[String]) -> Result<(), String> {
    let path = positional(args).ok_or("open needs a .shale path")?;
    let loaded = shale_core::read_project(std::path::Path::new(&path)).map_err(|err| err.to_string())?;
    println!(
        "{}\t{}\t{} model(s)",
        loaded.data.name,
        loaded.data.printer_id,
        loaded.data.models.len()
    );
    let _ = shale_core::remember_recent(
        &shale_core::config_dir(),
        &shale_core::RecentEntry {
            path: path.clone(),
            name: loaded.data.name,
            saved_at_ms: shale_core::now_ms(),
            printer: loaded.data.printer_id,
            missing: false,
        },
    );
    Ok(())
}

fn save_command(args: &[String]) -> Result<(), String> {
    let model = positional(args).ok_or("save needs a model path")?;
    let output = flag(args, "-o")
        .or_else(|| flag(args, "--output"))
        .ok_or("save needs -o PROJECT.shale")?;
    let bytes = std::fs::read(&model).map_err(|err| err.to_string())?;
    let name = std::path::Path::new(&model)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("model")
        .to_string();
    let catalog = catalog()?;
    let printer = flag(args, "--printer")
        .or_else(|| catalog.default_printer_id())
        .unwrap_or_else(|| "unknown".into());
    let data = shale_core::ProjectData {
        name: std::path::Path::new(&output)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("project")
            .to_string(),
        printer_id: printer.clone(),
        printer_label: printer.clone(),
        variant_name: flag(args, "--variant"),
        quality_type: flag(args, "--quality"),
        material_id: flag(args, "--material"),
        ams: Vec::new(),
        settings: UserSettings::default(),
        chord_mm: 0.1,
        turn_deg: 15.0,
        models: vec![shale_core::ProjectModel {
            name,
            bytes,
            transform: shale_core::Transform::default(),
            settings: std::collections::BTreeMap::new(),
            slot: None,
            plate: 0,
            painted: Vec::new(),
            blocked: Vec::new(),
        }],
        plate_names: vec!["plate 1".into()],
        instructions: Vec::new(),
        support: None,
        support_model: None,
        thumbnail: Vec::new(),
        dismissed_suggestions: Vec::new(),
        suggestions_shown: Vec::new(),
    };
    shale_core::write_project(std::path::Path::new(&output), &data).map_err(|err| err.to_string())?;
    let _ = shale_core::remember_recent(
        &shale_core::config_dir(),
        &shale_core::RecentEntry {
            path: output.clone(),
            name: data.name,
            saved_at_ms: shale_core::now_ms(),
            printer,
            missing: false,
        },
    );
    println!("{output}");
    Ok(())
}

fn stats_command(args: &[String]) -> Result<(), String> {
    let path = flag(args, "--gcode")
        .or_else(|| positional(args))
        .ok_or("stats needs --gcode PATH")?;
    let text = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let price = flag(args, "--price")
        .map(|value| value.parse::<f64>())
        .transpose()
        .map_err(|_| "price must be a number".to_string())?
        .unwrap_or(0.0);
    let stats = shale_core::slice_stats_from_gcode(&text, 1.75, price, has(args, "--layers"), Vec::new());
    if has(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&stats).map_err(|err| err.to_string())?);
        return Ok(());
    }
    println!("time {:.0}s", stats.print_time_seconds);
    println!("filament {:.2} m  {:.1} g  {:.2} {}", stats.filament_metres, stats.grams, stats.cost, stats.currency);
    println!(
        "walls {:.0}s  infill {:.0}s  supports {:.0}s  travel {:.0}s",
        stats.walls_seconds, stats.infill_seconds, stats.support_seconds, stats.travel_seconds
    );
    println!("layers {}  purge {:.1} g", stats.layers, stats.purge_grams);
    Ok(())
}

fn compare_command(args: &[String]) -> Result<(), String> {
    let model = positional(args).ok_or("compare needs a model path")?;
    let texts = many(args, "--patch");
    if texts.is_empty() {
        return Err("compare needs at least one --patch KEY=VALUE".into());
    }
    let patches = texts
        .iter()
        .map(|text| parse_patch(text))
        .collect::<Result<Vec<_>, _>>()?;
    let catalog = catalog()?;
    let engine = CliEngine::from_env();
    let mut session = shale_core::Session::new(catalog, Box::new(engine));
    if let Some(printer) = flag(args, "--printer") {
        session
            .set_printer(
                &printer,
                flag(args, "--variant"),
                flag(args, "--quality"),
                flag(args, "--material"),
            )
            .map_err(|err| err.to_string())?;
    }
    session
        .load_model(std::path::Path::new(&model))
        .map_err(|err| err.to_string())?;
    let concurrency = flag(args, "--concurrency")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| "concurrency must be a number".to_string())?;
    let price = flag(args, "--price")
        .map(|value| value.parse::<f64>())
        .transpose()
        .map_err(|_| "price must be a number".to_string())?;
    let rows = session
        .slice_variants(patches, concurrency, price)
        .map_err(|err| err.to_string())?;
    println!("index  time_s  metres  grams  cost  support_s  label");
    for row in rows {
        if let Some(error) = row.error {
            println!("{}\tfailed\t{error}", row.index);
            continue;
        }
        println!(
            "{}\t{:.0}\t{:.2}\t{:.1}\t{:.2}\t{:.0}\t{}",
            row.index,
            row.print_time_seconds.unwrap_or(0.0),
            row.filament_metres,
            row.grams,
            row.cost,
            row.support_seconds,
            row.label
        );
    }
    Ok(())
}

fn parse_patch(text: &str) -> Result<shale_core::SettingsPatch, String> {
    let mut patch = shale_core::SettingsPatch::default();
    for piece in text.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        let (key, value) = piece
            .split_once('=')
            .ok_or_else(|| format!("patch entry '{piece}' needs KEY=VALUE"))?;
        let number = |value: &str| {
            value
                .parse::<f64>()
                .map_err(|_| format!("'{value}' is not a number"))
        };
        match key.trim() {
            "layer_height" => patch.layer_height = Some(number(value)?),
            "infill" | "infill_sparse_density" => patch.infill_sparse_density = Some(number(value)?),
            "supports" | "support_enable" => {
                patch.support_enable = Some(!matches!(value, "0" | "false" | "off"));
            }
            "support_structure" => patch.support_structure = Some(value.to_string()),
            "adhesion_type" => patch.adhesion_type = Some(value.to_string()),
            "brim_width" => patch.brim_width = Some(number(value)?),
            other => {
                patch.settings.insert(other.to_string(), value.to_string());
            }
        }
    }
    Ok(patch)
}
