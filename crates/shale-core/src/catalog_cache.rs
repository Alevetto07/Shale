//! On-disk cache for the expensive parts of opening the profile library.
//!
//! Fingerprints come from the resource paths and the newest modification time
//! under each tree, so a vendor update or a moved install invalidates safely.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::{env, fs};

use serde::{Deserialize, Serialize};

use crate::bambu::{Library, MachineGcode};
use crate::definition::Definition;
use crate::error::Error;
use crate::instance::InstanceFile;
use crate::material::Material;

const CACHE_VERSION: u32 = 2;
const BAMBU_CACHE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct BambuLibraryCache {
    version: u32,
    fingerprint: u64,
    definitions: Vec<Definition>,
    instances: Vec<InstanceFile>,
    materials: Vec<Material>,
    gcode: Vec<(String, String, MachineGcode)>,
}

#[derive(Serialize, Deserialize)]
struct CuraExtrasCache {
    version: u32,
    fingerprint: u64,
    instances: Vec<InstanceFile>,
    materials: Vec<Material>,
}

pub(crate) fn cache_dir() -> PathBuf {
    if let Ok(dir) = env::var("SHALE_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    if let Ok(local) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Shale").join("cache");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".cache").join("shale");
    }
    env::temp_dir().join("shale-cache")
}

pub(crate) fn fingerprint(cura: &Path, materials: &Path, resources: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    cura.display().to_string().hash(&mut hasher);
    materials.display().to_string().hash(&mut hasher);
    RESOURCE_VERSION.hash(&mut hasher);
    max_mtime(resources).hash(&mut hasher);
    max_mtime(materials).hash(&mut hasher);
    if let Some(orca) = crate::bambu::profiles_root() {
        orca.display().to_string().hash(&mut hasher);
        max_mtime(&orca).hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn load_cura_extras(
    cura: &Path,
    materials: &Path,
    resources: &Path,
) -> Result<Option<(Vec<InstanceFile>, Vec<Material>)>, Error> {
    let fingerprint = fingerprint(cura, materials, resources);
    let file = cache_dir().join(format!("cura-extras-{fingerprint:x}.json"));
    if !file.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&file).map_err(|err| Error::message(err.to_string()))?;
    let cached: CuraExtrasCache =
        serde_json::from_str(&text).map_err(|err| Error::message(err.to_string()))?;
    if cached.version != CACHE_VERSION || cached.fingerprint != fingerprint {
        return Ok(None);
    }
    Ok(Some((cached.instances, cached.materials)))
}

pub(crate) fn store_cura_extras(
    cura: &Path,
    materials: &Path,
    resources: &Path,
    instances: &[InstanceFile],
    material_rows: &[Material],
) -> Result<(), Error> {
    let fingerprint = fingerprint(cura, materials, resources);
    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|err| Error::message(err.to_string()))?;
    let body = CuraExtrasCache {
        version: CACHE_VERSION,
        fingerprint,
        instances: instances.to_vec(),
        materials: material_rows.to_vec(),
    };
    let json = serde_json::to_vec(&body).map_err(|err| Error::message(err.to_string()))?;
    let file = dir.join(format!("cura-extras-{fingerprint:x}.json"));
    fs::write(&file, json).map_err(|err| Error::message(err.to_string()))?;
    Ok(())
}

fn max_mtime(root: &Path) -> u64 {
    let mut best = metadata_epoch(root);
    let mut stack = vec![root.to_path_buf()];
    let mut seen = 0u64;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            best = best.max(metadata_epoch(&path));
            seen += 1;
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
                    continue;
                }
                stack.push(path);
            }
            if seen > 50_000 {
                return best;
            }
        }
    }
    best
}

fn metadata_epoch(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

const RESOURCE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) fn bambu_fingerprint(root: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    root.display().to_string().hash(&mut hasher);
    RESOURCE_VERSION.hash(&mut hasher);
    max_mtime(root).hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn load_bambu_library(root: &Path) -> Option<Library> {
    let fingerprint = bambu_fingerprint(root);
    let file = cache_dir().join(format!("bambu-library-{fingerprint:x}.json"));
    if !file.is_file() {
        return None;
    }
    let text = fs::read_to_string(&file).ok()?;
    let cached: BambuLibraryCache = serde_json::from_str(&text).ok()?;
    if cached.version != BAMBU_CACHE_VERSION || cached.fingerprint != fingerprint {
        return None;
    }
    Some(Library {
        definitions: cached.definitions,
        instances: cached.instances,
        materials: cached.materials,
        gcode: cached.gcode,
    })
}

pub(crate) fn store_bambu_library(root: &Path, library: &Library) -> Result<(), Error> {
    let fingerprint = bambu_fingerprint(root);
    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|err| Error::message(err.to_string()))?;
    let body = BambuLibraryCache {
        version: BAMBU_CACHE_VERSION,
        fingerprint,
        definitions: library.definitions.clone(),
        instances: library.instances.clone(),
        materials: library.materials.clone(),
        gcode: library.gcode.clone(),
    };
    let json = serde_json::to_vec(&body).map_err(|err| Error::message(err.to_string()))?;
    let file = dir.join(format!("bambu-library-{fingerprint:x}.json"));
    fs::write(&file, json).map_err(|err| Error::message(err.to_string()))?;
    Ok(())
}