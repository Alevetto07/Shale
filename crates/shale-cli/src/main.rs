//! `shale` — the same engine the window drives, without the window.
//!
//! No Tauri, no webview, no Node: this binary opens the Cura resource tree,
//! resolves a printer the way the app does, hands the mesh to an unmodified
//! CuraEngine, and writes a G-code file. It is meant to be copied to a machine
//! that has no desktop at all, so every path can be given by environment
//! variable and every failure names the path it tried.

use std::path::PathBuf;
use std::process::ExitCode;

use shale_core::catalog::{Catalog, ResourcePaths};
use shale_core::engine::{CliEngine, EngineProgress};
use shale_core::job::{slice, PlacedModel};
use shale_core::mesh::{load_mesh, place_on_bed};
use shale_core::resolver::{ResolveRequest, UserSettings};

const USAGE: &str = "\
shale — slice a model without the window

  shale info                     resources, engine and catalogue
  shale printers [--search TEXT]  printers the catalogue knows
  shale settings --printer ID [--variant NAME] [--quality TYPE] [--material ID]
  shale slice MODEL [options]    write G-code next to the model, or -o PATH

slice options
  --printer ID        printer definition id (default: the first one found)
  --variant NAME      nozzle variant, e.g. \"0.4 mm\"
  --quality TYPE      quality profile type, e.g. \"standard\"
  --material ID       filament id, e.g. \"generic_pla\"
  --layer MM          layer height
  --infill PERCENT    sparse infill density
  --supports          enable support
  --set KEY=VALUE     any Cura setting, repeatable
  -o, --output PATH   where to write the G-code

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

fn env_line(name: &str, configured: bool, detail: &str) {
    if configured {
        println!("{name:<11} set ({detail})");
    } else {
        println!("{name:<11} missing — {detail}");
    }
}

fn info() -> Result<(), String> {
    let engine = CliEngine::from_env();
    println!("shale {}", env!("CARGO_PKG_VERSION"));
    println!("engine   {}", engine.program.display());
    match engine.probe() {
        Ok(()) => println!("engine   ok"),
        Err(message) => println!("engine   not usable: {message}"),
    }
    if let Ok(path) = std::env::var("SHALE_CURAENGINE") {
        env_line("SHALE_CURAENGINE", true, &path);
    } else {
        let usable = engine.probe();
        let detail = format!("not set (auto-detected: {})", engine.program.display());
        env_line("SHALE_CURAENGINE", usable.is_ok(), &detail);
        if usable.is_err() {
            eprintln!(
                "shale: set SHALE_CURAENGINE to the CuraEngine binary, or install UltiMaker Cura 5.x"
            );
        }
    }
    if let Ok(path) = std::env::var("SHALE_CURAENGINE_LIB_DIR") {
        env_line("SHALE_CURAENGINE_LIB_DIR", true, &path);
    } else {
        env_line(
            "SHALE_CURAENGINE_LIB_DIR",
            false,
            "optional; defaults to the folder containing the engine",
        );
    }
    if let Ok(path) = std::env::var("SHALE_CURA_DIR") {
        env_line("SHALE_CURA_DIR", true, &path);
    } else {
        env_line("SHALE_CURA_DIR", false, "set with SHALE_MATERIALS_DIR");
    }
    if let Ok(path) = std::env::var("SHALE_MATERIALS_DIR") {
        env_line("SHALE_MATERIALS_DIR", true, &path);
    } else {
        env_line("SHALE_MATERIALS_DIR", false, "set with SHALE_CURA_DIR");
    }
    match std::env::var("SHALE_ORCA_DIR") {
        Ok(path) => env_line("SHALE_ORCA_DIR", true, &path),
        Err(_) => match shale_core::bambu::profiles_root() {
            Some(path) => env_line(
                "SHALE_ORCA_DIR",
                false,
                &format!("not set (auto-detected: {})", path.display()),
            ),
            None => env_line(
                "SHALE_ORCA_DIR",
                false,
                "missing — set to Bambu/Orca profiles or run scripts/fetch-resources.sh",
            ),
        },
    }
    match ResourcePaths::discover() {
        Ok(paths) => {
            println!("cura     {}", paths.cura.display());
            println!("materials {}", paths.materials.display());
            match Catalog::open(paths) {
                Ok(catalog) => {
                    let printers = catalog.selectable_printers();
                    println!("printers {}", printers.len());
                }
                Err(err) => println!("catalog  unreadable: {err}"),
            }
        }
        Err(err) => println!("cura     not found: {err}"),
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
        },
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
    let mesh = load_mesh(&model_path).map_err(|err| err.to_string())?;

    let catalog = catalog()?;
    let printer = flag(args, "--printer").unwrap_or(default_printer(&catalog)?);
    let request = resolve_request(args, &printer);
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

    let transform = place_on_bed(&mesh, width, depth, false);
    let placed = vec![PlacedModel {
        name: model_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "model".to_string()),
        mesh,
        transform,
        settings: Vec::new(),
        slot: None,
    }];

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
        output.clone(),
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
    Ok(())
}
