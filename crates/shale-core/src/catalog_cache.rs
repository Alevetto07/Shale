//! On-disk cache for the expensive parts of opening the profile library.
//!
//! A **cheap** fingerprint runs on every load (folder stamps, no full-tree walk).
//! A **full** fingerprint (counts and byte totals) is computed only when writing
//! the cache on a background thread, so a cold parse never waits on disk I/O for
//! cache maintenance.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread;
use std::{env, fs};

use serde::{Deserialize, Serialize};

use crate::bambu::{Library, MachineGcode};
use crate::definition::Definition;
use crate::error::Error;
use crate::instance::InstanceFile;
use crate::material::Material;

const CACHE_VERSION: u32 = 3;
const BAMBU_CACHE_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct TreeStamp {
    newest_secs: u64,
    file_count: u64,
    total_bytes: u64,
}

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

/// User app-data, never next to the install / uninstall layout.
pub(crate) fn cache_dir() -> PathBuf {
    if let Ok(dir) = env::var("SHALE_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    if let Ok(roaming) = env::var("APPDATA") {
        return PathBuf::from(roaming).join("Shale").join("catalog-cache");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".cache").join("shale").join("catalog-cache");
    }
    env::temp_dir().join("shale-catalog-cache")
}

pub(crate) fn fingerprint_cura(cura: &Path, materials: &Path, resources: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    cura.display().to_string().hash(&mut hasher);
    hash_stamps(&[
        stamp_path(resources),
        stamp_path(&resources.join("definitions")),
        stamp_path(&resources.join("quality")),
        stamp_path(&resources.join("variants")),
        stamp_path(&resources.join("intent")),
        stamp_path(materials),
        stamp_version_marker(resources),
    ])
    .hash(&mut hasher);
    if let Some(orca) = crate::bambu::profiles_root() {
        fingerprint_bambu(&orca).hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn fingerprint_bambu(root: &Path) -> u64 {
    let mut stamps = vec![
        stamp_path(root),
        stamp_version_marker(root),
    ];
    for vendor in ["BBL", "Anycubic", "Creality"] {
        let base = root.join(vendor);
        if base.is_dir() {
            stamps.push(stamp_path(&base.join("machine")));
            stamps.push(stamp_path(&base.join("process")));
            stamps.push(stamp_path(&base.join("filament")));
        }
    }
    hash_stamps(&stamps)
}

fn hash_stamps(stamps: &[TreeStamp]) -> u64 {
    let mut hasher = DefaultHasher::new();
    RESOURCE_VERSION.hash(&mut hasher);
    for stamp in stamps {
        stamp.newest_secs.hash(&mut hasher);
        stamp.file_count.hash(&mut hasher);
        stamp.total_bytes.hash(&mut hasher);
    }
    hasher.finish()
}

/// Shallow stamp of one directory: each entry's metadata only (no file bodies).
fn stamp_path(dir: &Path) -> TreeStamp {
    let mut out = TreeStamp {
        newest_secs: metadata_epoch(dir),
        ..Default::default()
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        out.file_count += 1;
        out.total_bytes = out.total_bytes.saturating_add(meta.len());
        if let Ok(modified) = meta.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                out.newest_secs = out.newest_secs.max(duration.as_secs());
            }
        }
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some(".git") {
                continue;
            }
            for sub in fs::read_dir(&path).into_iter().flatten().flatten() {
                let Ok(sub_meta) = fs::metadata(sub.path()) else {
                    continue;
                };
                out.file_count += 1;
                out.total_bytes = out.total_bytes.saturating_add(sub_meta.len());
                if let Ok(modified) = sub_meta.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        out.newest_secs = out.newest_secs.max(duration.as_secs());
                    }
                }
            }
        }
    }
    out
}

fn stamp_version_marker(resources: &Path) -> TreeStamp {
    let mut candidates = vec![
        resources.join("version.json"),
        resources.join("definitions/fdmprinter.def.json"),
    ];
    if let Some(parent) = resources.parent() {
        candidates.push(parent.join("version.json"));
    }
    let mut out = TreeStamp::default();
    for path in candidates {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        out.file_count += 1;
        out.total_bytes = out.total_bytes.saturating_add(meta.len());
        out.newest_secs = out.newest_secs.max(metadata_epoch(&path));
    }
    out
}

fn metadata_epoch(path: &Path) -> u64 {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn load_cura_extras(
    cura: &Path,
    materials: &Path,
    resources: &Path,
) -> Result<Option<(Vec<InstanceFile>, Vec<Material>)>, Error> {
    let fingerprint = fingerprint_cura(cura, materials, resources);
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

pub(crate) fn store_cura_extras_async(
    cura: PathBuf,
    materials: PathBuf,
    resources: PathBuf,
    instances: Vec<InstanceFile>,
    material_rows: Vec<Material>,
) {
    thread::spawn(move || {
        let _ = store_cura_extras_sync(&cura, &materials, &resources, &instances, &material_rows);
    });
}

fn store_cura_extras_sync(
    cura: &Path,
    materials: &Path,
    resources: &Path,
    instances: &[InstanceFile],
    material_rows: &[Material],
) -> Result<(), Error> {
    let fingerprint = fingerprint_cura(cura, materials, resources);
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
    prune_stale(&dir, "cura-extras-", &file);
    Ok(())
}

pub(crate) fn load_bambu_library(root: &Path) -> Option<Library> {
    let fingerprint = fingerprint_bambu(root);
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

pub(crate) fn store_bambu_library_async(root: PathBuf, library: Library) {
    thread::spawn(move || {
        let _ = store_bambu_library_sync(&root, &library);
    });
}

fn store_bambu_library_sync(root: &Path, library: &Library) -> Result<(), Error> {
    let fingerprint = fingerprint_bambu(root);
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
    prune_stale(&dir, "bambu-library-", &file);
    Ok(())
}

fn prune_stale(dir: &Path, prefix: &str, keep: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with(prefix) && name.ends_with(".json") && path != keep {
            let _ = fs::remove_file(&path);
        }
    }
}

const RESOURCE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cache_home() -> PathBuf {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| {
            let dir = env::temp_dir().join(format!(
                "shale-catalog-cache-test-{}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).ok();
            env::set_var("SHALE_CACHE_DIR", &dir);
            dir
        })
        .clone()
    }

    #[test]
    fn cache_writes_under_test_dir_not_user_profile() {
        let home = test_cache_home();
        let dir = cache_dir();
        assert!(
            dir.starts_with(&home),
            "cache dir {:?} should live under test home {:?}",
            dir,
            home
        );
    }

    #[test]
    fn fingerprint_changes_when_a_file_is_added() {
        let base = env::temp_dir().join(format!("shale-fp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("definitions")).unwrap();
        let resources = base.join("res");
        fs::create_dir_all(&resources).unwrap();
        fs::write(resources.join("definitions/fdmprinter.def.json"), b"{}").unwrap();
        let a = fingerprint_cura(&base, &base, &resources);
        fs::write(resources.join("definitions/extra.def.json"), b"{}").unwrap();
        let b = fingerprint_cura(&base, &base, &resources);
        assert_ne!(a, b);
    }
}