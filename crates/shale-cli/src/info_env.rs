//! Lines printed by `shale info` for environment variables.

use shale_core::catalog::ResourcePaths;
use shale_core::Error;

/// How a resource path should be reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvPath {
    /// The variable is set in the environment.
    Set(String),
    /// Shale found a usable path without the variable.
    AutoDetected(String),
    /// The variable is unset but the default is acceptable.
    UnsetOptional(&'static str),
    /// Nothing usable was found.
    NotFound(&'static str),
}

const NAME_WIDTH: usize = 26;

fn format_path_for_display(path: &str) -> String {
    #[cfg(windows)]
    {
        path.replace('/', "\\")
    }
    #[cfg(not(windows))]
    path.to_string()
}

pub fn env_path_line(name: &str, path: EnvPath) -> String {
    match path {
        EnvPath::Set(value) => format!("{name:<NAME_WIDTH$}set: {value}"),
        EnvPath::AutoDetected(value) => {
            let value = format_path_for_display(&value);
            format!("{name:<NAME_WIDTH$}not set (auto-detected: {value})")
        }
        EnvPath::UnsetOptional(hint) => format!("{name:<NAME_WIDTH$}not set ({hint})"),
        EnvPath::NotFound(hint) => format!("{name:<NAME_WIDTH$}not found — {hint}"),
    }
}

pub fn cura_engine_path() -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_CURAENGINE") {
        return EnvPath::Set(path);
    }
    let engine = shale_core::engine::CliEngine::from_env();
    if engine.probe().is_ok() {
        EnvPath::AutoDetected(engine.program.display().to_string())
    } else {
        EnvPath::NotFound(cura_engine_not_found_hint())
    }
}

#[cfg(windows)]
fn cura_engine_not_found_hint() -> &'static str {
    "set SHALE_CURAENGINE to CuraEngine.exe, or install UltiMaker Cura 5.x under Program Files"
}

#[cfg(not(windows))]
fn cura_engine_not_found_hint() -> &'static str {
    "set SHALE_CURAENGINE to the CuraEngine binary, or install UltiMaker Cura 5.x"
}

pub fn cura_resources_path_from(discovered: &Result<ResourcePaths, Error>) -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_CURA_DIR") {
        return EnvPath::Set(path);
    }
    match discovered {
        Ok(paths) => EnvPath::AutoDetected(paths.cura.display().to_string()),
        Err(_) => EnvPath::NotFound(cura_catalog_not_found_hint()),
    }
}

pub fn materials_path_from(discovered: &Result<ResourcePaths, Error>) -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_MATERIALS_DIR") {
        return EnvPath::Set(path);
    }
    match discovered {
        Ok(paths) => EnvPath::AutoDetected(paths.materials.display().to_string()),
        Err(_) => EnvPath::NotFound(cura_catalog_not_found_hint()),
    }
}

fn cura_catalog_not_found_hint() -> &'static str {
    "set SHALE_CURA_DIR and SHALE_MATERIALS_DIR, or run scripts/fetch-resources.sh"
}

pub fn orca_profiles_path() -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_ORCA_DIR") {
        return EnvPath::Set(format_path_for_display(&path));
    }
    match shale_core::bambu::profiles_root() {
        Some(path) => EnvPath::AutoDetected(format_path_for_display(&path.display().to_string())),
        None => EnvPath::NotFound(
            "set SHALE_ORCA_DIR to Bambu/Orca profiles, or run scripts/fetch-resources.sh",
        ),
    }
}

pub fn cura_engine_lib_path() -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_CURAENGINE_LIB_DIR") {
        EnvPath::Set(path)
    } else {
        EnvPath::UnsetOptional("optional; uses the engine's folder")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_line_format() {
        let line = env_path_line("SHALE_CURA_DIR", EnvPath::Set("/cura".into()));
        assert!(line.starts_with("SHALE_CURA_DIR"));
        assert!(line.contains("set: /cura"));
    }

    #[test]
    fn auto_detected_line_format() {
        let line = env_path_line(
            "SHALE_ORCA_DIR",
            EnvPath::AutoDetected("C:\\profiles".into()),
        );
        assert!(line.contains("not set (auto-detected: C:\\profiles)"));
        assert!(!line.contains("missing"));
        let value = line.get(NAME_WIDTH..).unwrap_or("").trim_start();
        assert!(
            value.starts_with("not set (auto-detected:"),
            "value should be auto-detected, got {:?}",
            value
        );
    }

    #[test]
    fn unset_optional_line_format() {
        let line = env_path_line(
            "SHALE_CURAENGINE_LIB_DIR",
            EnvPath::UnsetOptional("optional; uses the engine's folder"),
        );
        assert!(line.contains("not set (optional; uses the engine's folder)"));
    }

    #[test]
    fn not_found_line_format() {
        let line = env_path_line(
            "SHALE_CURAENGINE",
            EnvPath::NotFound("install UltiMaker Cura 5.x"),
        );
        assert!(line.starts_with("SHALE_CURAENGINE"));
        assert!(line.contains("not found — install UltiMaker Cura 5.x"));
        assert!(!line.contains("missing — missing"));
    }
}
