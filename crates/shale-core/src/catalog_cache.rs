//! On-disk cache for the expensive parts of opening the profile library.
//!
//! **Load path:** path-key lookup and at most one marker `stat` — no tree walks.
//! **Write path:** fingerprint and JSON serialization run on a background thread.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::thread;
use std::time::{Duration, SystemTime};
use std::{env, fs};

use serde::{Deserialize, Serialize};

use crate::bambu::{Library, MachineGcode};
use crate::definition::Definition;
use crate::error::Error;
use crate::instance::InstanceFile;
use crate::material::Material;

const CACHE_VERSION: u32 = 4;
const BAMBU_CACHE_VERSION: u32 = 3;
const ENV_CACHE_DIR: &str = "SHALE_CACHE_DIR";

#[cfg(test)]
static TEST_CACHE_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct TreeStamp {
    newest_secs: u64,
    file_count: u64,
    total_bytes: u64,
}

#[derive(Serialize, Deserialize)]
struct BambuLibraryCache {
    version: u32,
    marker_epoch: u64,
    definitions: Vec<Definition>,
    instances: Vec<InstanceFile>,
    materials: Vec<Material>,
    gcode: Vec<(String, String, MachineGcode)>,
}

#[derive(Serialize, Deserialize)]
struct CuraExtrasCache {
    version: u32,
    marker_epoch: u64,
    instances: Vec<InstanceFile>,
    materials: Vec<Material>,
}

/// User app-data, never next to the install / uninstall layout.
pub(crate) fn cache_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Ok(guard) = TEST_CACHE_DIR.read() {
            if let Some(dir) = guard.as_ref() {
                return dir.clone();
            }
        }
        let dir = env::temp_dir().join(format!("shale-catalog-cache-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        if let Ok(mut guard) = TEST_CACHE_DIR.write() {
            if guard.is_none() {
                *guard = Some(dir.clone());
            }
            if let Some(set) = guard.as_ref() {
                return set.clone();
            }
        }
        return dir;
    }
    #[cfg(not(test))]
    {
        if let Ok(dir) = env::var(ENV_CACHE_DIR) {
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
}

#[cfg(test)]
pub(crate) fn set_test_cache_dir(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    if let Ok(mut guard) = TEST_CACHE_DIR.write() {
        *guard = Some(dir);
    }
}

#[cfg(test)]
pub(crate) fn reset_test_cache_dir() {
    if let Ok(mut guard) = TEST_CACHE_DIR.write() {
        *guard = None;
    }
}

pub(crate) fn remove_legacy_cache_dir() {
    #[cfg(windows)]
    if let Ok(local) = env::var("LOCALAPPDATA") {
        let legacy = PathBuf::from(local).join("Shale").join("cache");
        let _ = fs::remove_dir_all(&legacy);
    }
}

fn path_key(parts: &[&Path]) -> u64 {
    let mut hasher = DefaultHasher::new();
    RESOURCE_VERSION.hash(&mut hasher);
    for part in parts {
        part.display().to_string().hash(&mut hasher);
    }
    hasher.finish()
}

fn cura_marker_epoch(resources: &Path) -> u64 {
    metadata_epoch(
        &resources
            .join("definitions")
            .join("fdmprinter.def.json"),
    )
}

fn bambu_marker_epoch(root: &Path) -> u64 {
    let marker = root.join("BBL").join("machine");
    if marker.is_dir() {
        metadata_epoch(&marker)
    } else {
        metadata_epoch(root)
    }
}

pub(crate) fn load_cura_extras(
    cura: &Path,
    materials: &Path,
    resources: &Path,
) -> Result<Option<(Vec<InstanceFile>, Vec<Material>)>, Error> {
    let key = path_key(&[cura, materials]);
    let file = cache_dir().join(format!("cura-extras-{key:x}.json"));
    if !file.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&file).map_err(|err| Error::message(err.to_string()))?;
    let cached: CuraExtrasCache =
        serde_json::from_str(&text).map_err(|err| Error::message(err.to_string()))?;
    if cached.version != CACHE_VERSION {
        return Ok(None);
    }
    if cached.marker_epoch != cura_marker_epoch(resources) {
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
    let key = path_key(&[cura, materials]);
    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|err| Error::message(err.to_string()))?;
    let body = CuraExtrasCache {
        version: CACHE_VERSION,
        marker_epoch: cura_marker_epoch(resources),
        instances: instances.to_vec(),
        materials: material_rows.to_vec(),
    };
    let json = serde_json::to_vec(&body).map_err(|err| Error::message(err.to_string()))?;
    let file = dir.join(format!("cura-extras-{key:x}.json"));
    fs::write(&file, json).map_err(|err| Error::message(err.to_string()))?;
    prune_stale(&dir, "cura-extras-", &file);
    Ok(())
}

pub(crate) fn load_bambu_library(root: &Path) -> Option<Library> {
    let key = path_key(&[root]);
    let file = cache_dir().join(format!("bambu-library-{key:x}.json"));
    if !file.is_file() {
        return None;
    }
    let text = fs::read_to_string(&file).ok()?;
    let cached: BambuLibraryCache = serde_json::from_str(&text).ok()?;
    if cached.version != BAMBU_CACHE_VERSION {
        return None;
    }
    if cached.marker_epoch != bambu_marker_epoch(root) {
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
    let key = path_key(&[root]);
    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|err| Error::message(err.to_string()))?;
    let body = BambuLibraryCache {
        version: BAMBU_CACHE_VERSION,
        marker_epoch: bambu_marker_epoch(root),
        definitions: library.definitions.clone(),
        instances: library.instances.clone(),
        materials: library.materials.clone(),
        gcode: library.gcode.clone(),
    };
    let json = serde_json::to_vec(&body).map_err(|err| Error::message(err.to_string()))?;
    let file = dir.join(format!("bambu-library-{key:x}.json"));
    fs::write(&file, json).map_err(|err| Error::message(err.to_string()))?;
    prune_stale(&dir, "bambu-library-", &file);
    Ok(())
}

fn prune_stale(dir: &Path, prefix: &str, keep: &Path) {
    let young = SystemTime::now()
        .checked_sub(Duration::from_secs(60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(prefix) || !name.ends_with(".json") {
            continue;
        }
        let modified = fs::metadata(&path)
            .ok()
            .and_then(|meta| meta.modified().ok());
        if modified.is_some_and(|time| time >= young) {
            continue;
        }
        let _ = fs::remove_file(&path);
    }
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

/// Used only by cache regression tests on the fixture tree (no Bambu install scan).
#[cfg(test)]
pub(crate) fn fingerprint_fixture(resources: &Path, materials: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_stamp_tree(&mut hasher, resources);
    hash_stamp_tree(&mut hasher, materials);
    hasher.finish()
}

#[cfg(test)]
fn hash_stamp_tree(hasher: &mut DefaultHasher, dir: &Path) {
    RESOURCE_VERSION.hash(hasher);
    dir.display().to_string().hash(hasher);
    let stamp = stamp_fixture_dir(dir);
    stamp.newest_secs.hash(hasher);
    stamp.file_count.hash(hasher);
    stamp.total_bytes.hash(hasher);
}

#[cfg(test)]
fn stamp_fixture_dir(dir: &Path) -> TreeStamp {
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
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn isolated_cache() -> PathBuf {
        static DIR: OnceLock<PathBuf> = OnceLock::new();
        DIR.get_or_init(|| {
            let dir = env::temp_dir().join(format!(
                "shale-catalog-cache-unit-{}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).ok();
            set_test_cache_dir(dir.clone());
            dir
        })
        .clone()
    }

    #[test]
    fn cache_writes_under_test_dir_not_user_profile() {
        let home = isolated_cache();
        let dir = cache_dir();
        assert!(dir.starts_with(&home), "cache {:?} vs {:?}", dir, home);
    }

    #[test]
    fn fingerprint_changes_when_a_file_is_added() {
        let base = env::temp_dir().join(format!("shale-fp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let materials = base.join("materials");
        let resources = base.join("cura");
        fs::create_dir_all(resources.join("definitions")).unwrap();
        fs::create_dir_all(&materials).unwrap();
        fs::write(
            resources.join("definitions/fdmprinter.def.json"),
            b"{}",
        )
        .unwrap();
        let a = fingerprint_fixture(&resources, &materials);
        fs::write(resources.join("definitions/extra.def.json"), b"{}").unwrap();
        let b = fingerprint_fixture(&resources, &materials);
        assert_ne!(a, b);
    }
}