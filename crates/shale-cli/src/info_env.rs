//! Lines printed by `shale info` for environment variables.

/// How a resource path should be reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvPath {
    /// The variable is set in the environment.
    Set(String),
    /// Shale found a usable path without the variable.
    AutoDetected(String),
    /// Nothing usable was found.
    NotFound(&'static str),
}

pub fn env_path_line(name: &str, path: EnvPath) -> String {
    match path {
        EnvPath::Set(value) => format!("{name:<22} set: {value}"),
        EnvPath::AutoDetected(value) => format!("{name:<22} not set (auto-detected: {value})"),
        EnvPath::NotFound(hint) => format!("{name:<22} not found — {hint}"),
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
        EnvPath::NotFound(
            "set SHALE_CURAENGINE to the CuraEngine binary, or install UltiMaker Cura 5.x",
        )
    }
}

pub fn cura_resources_path() -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_CURA_DIR") {
        return EnvPath::Set(path);
    }
    match shale_core::catalog::ResourcePaths::discover() {
        Ok(paths) => EnvPath::AutoDetected(paths.cura.display().to_string()),
        Err(_) => EnvPath::NotFound(
            "set SHALE_CURA_DIR and SHALE_MATERIALS_DIR, or run scripts/fetch-resources.sh",
        ),
    }
}

pub fn materials_path() -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_MATERIALS_DIR") {
        return EnvPath::Set(path);
    }
    match shale_core::catalog::ResourcePaths::discover() {
        Ok(paths) => EnvPath::AutoDetected(paths.materials.display().to_string()),
        Err(_) => EnvPath::NotFound(
            "set SHALE_MATERIALS_DIR with SHALE_CURA_DIR, or run scripts/fetch-resources.sh",
        ),
    }
}

pub fn orca_profiles_path() -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_ORCA_DIR") {
        return EnvPath::Set(path);
    }
    match shale_core::bambu::profiles_root() {
        Some(path) => EnvPath::AutoDetected(path.display().to_string()),
        None => EnvPath::NotFound(
            "set SHALE_ORCA_DIR to Bambu/Orca profiles, or run scripts/fetch-resources.sh",
        ),
    }
}

pub fn cura_engine_lib_path() -> EnvPath {
    if let Ok(path) = std::env::var("SHALE_CURAENGINE_LIB_DIR") {
        EnvPath::Set(path)
    } else {
        EnvPath::NotFound("optional; defaults to the folder containing the engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_line_format() {
        let line = env_path_line("SHALE_CURA_DIR", EnvPath::Set("/cura".into()));
        assert_eq!(line, "SHALE_CURA_DIR         set: /cura");
    }

    #[test]
    fn auto_detected_line_format() {
        let line = env_path_line(
            "SHALE_ORCA_DIR",
            EnvPath::AutoDetected("C:\\profiles".into()),
        );
        assert!(line.contains("not set (auto-detected: C:\\profiles)"));
        assert!(!line.contains("missing"));
        assert!(!line.contains("set ("));
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