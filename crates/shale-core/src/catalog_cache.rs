//! On-disk cache for the expensive parts of opening the profile library.
//!
//! **Load path:** path-key lookup and cheap source markers — no full-tree walks.
//! **Write path:** JSON serialization on a background thread; prune never deletes
//! other sources' cache files.

use std::cell::RefCell;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::{env, fs};

thread_local! {
    static CACHE_DIR_THREAD: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

use serde::{Deserialize, Serialize};

use crate::bambu::{Library, MachineGcode};
use crate::definition::Definition;
use crate::error::Error;
use crate::instance::InstanceFile;
use crate::material::Material;

const CACHE_VERSION: u32 = 6;
const BAMBU_CACHE_VERSION: u32 = 4;

static CACHE_DIR_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct FileSig {
    mtime_secs: u64,
    size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CacheMarkers {
    entries: Vec<(String, FileSig)>,
}

#[derive(Serialize, Deserialize)]
struct BambuLibraryCache {
    version: u32,
    markers: CacheMarkers,
    definitions: Vec<Definition>,
    instances: Vec<InstanceFile>,
    materials: Vec<Material>,
    gcode: Vec<(String, String, MachineGcode)>,
}

#[derive(Clone, Debug)]
pub(crate) struct CuraSnapshot {
    pub definitions: HashMap<String, Definition>,
    pub instances: Vec<InstanceFile>,
    pub materials: Vec<Material>,
}

#[derive(Serialize, Deserialize)]
struct CuraExtrasCache {
    version: u32,
    markers: CacheMarkers,
    #[serde(default)]
    definitions: Vec<Definition>,
    instances: Vec<InstanceFile>,
    materials: Vec<Material>,
}

/// Sets the on-disk catalogue cache directory for this process (integration tests).
pub fn set_cache_dir_override(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    if let Ok(mut guard) = CACHE_DIR_OVERRIDE.write() {
        *guard = Some(dir);
    }
}

#[cfg(test)]
pub(crate) fn set_cache_dir_thread_local(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    CACHE_DIR_THREAD.with(|slot| *slot.borrow_mut() = Some(dir));
}

#[cfg(test)]
pub(crate) fn clear_cache_dir_thread_local() {
    CACHE_DIR_THREAD.with(|slot| *slot.borrow_mut() = None);
}

fn test_mode_active() -> bool {
    cfg!(test) || env::var_os("SHALE_TEST").is_some_and(|value| value != "0" && !value.is_empty())
}

fn isolated_test_cache_dir() -> PathBuf {
    let dir = env::temp_dir().join(format!("shale-catalog-cache-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

/// User app-data, never next to the install / uninstall layout.
pub(crate) fn cache_dir() -> PathBuf {
    if let Some(dir) = CACHE_DIR_THREAD.with(|slot| slot.borrow().clone()) {
        return dir;
    }
    if let Ok(guard) = CACHE_DIR_OVERRIDE.read() {
        if let Some(dir) = guard.as_ref() {
            return dir.clone();
        }
    }
    if let Ok(dir) = env::var("SHALE_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    if test_mode_active() {
        return isolated_test_cache_dir();
    }
    #[cfg(windows)]
    if let Ok(roaming) = env::var("APPDATA") {
        return PathBuf::from(roaming).join("Shale").join("catalog-cache");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join(".cache")
            .join("shale")
            .join("catalog-cache");
    }
    env::temp_dir().join("shale-catalog-cache")
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

fn file_sig(path: &Path) -> Option<FileSig> {
    let meta = fs::metadata(path).ok()?;
    let mtime_secs = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    Some(FileSig {
        mtime_secs,
        size: meta.len(),
    })
}

fn push_sig(markers: &mut CacheMarkers, label: impl Into<String>, path: &Path) {
    if let Some(sig) = file_sig(path) {
        markers.entries.push((label.into(), sig));
    }
}

/// Immediate children only — catches rewrites and new sibling files without deep walks.
fn push_shallow_dir(markers: &mut CacheMarkers, label: &str, dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut count = 0u64;
    let mut bytes = 0u64;
    let mut newest = 0u64;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        count += 1;
        bytes = bytes.saturating_add(meta.len());
        if let Ok(modified) = meta.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                newest = newest.max(duration.as_secs());
            }
        }
    }
    markers.entries.push((
        format!("{label}:shallow"),
        FileSig {
            mtime_secs: newest,
            size: bytes.wrapping_add(count),
        },
    ));
}

fn cura_markers(resources: &Path, materials: &Path) -> CacheMarkers {
    let mut out = CacheMarkers {
        entries: Vec::new(),
    };
    push_sig(
        &mut out,
        "fdmprinter",
        &resources.join("definitions/fdmprinter.def.json"),
    );
    push_shallow_dir(&mut out, "definitions", &resources.join("definitions"));
    push_sig(
        &mut out,
        "fdmextruder",
        &resources.join("extruders/fdmextruder.def.json"),
    );
    for name in ["Version.version", "version.semver"] {
        let path = resources.join(name);
        if path.is_file() {
            push_sig(&mut out, name, &path);
            break;
        }
    }
    push_shallow_dir(&mut out, "quality", &resources.join("quality"));
    push_shallow_dir(&mut out, "variants", &resources.join("variants"));
    push_shallow_dir(&mut out, "materials", materials);
    out
}

fn bambu_markers(root: &Path) -> CacheMarkers {
    let mut out = CacheMarkers {
        entries: Vec::new(),
    };
    push_sig(&mut out, "BBL.json", &root.join("BBL.json"));
    for name in ["Version.json", "version.json"] {
        let path = root.join(name);
        if path.is_file() {
            push_sig(&mut out, name, &path);
            break;
        }
    }
    let bbl = root.join("BBL");
    if bbl.is_dir() {
        push_shallow_dir(&mut out, "BBL/machine", &bbl.join("machine"));
        push_shallow_dir(&mut out, "BBL/process", &bbl.join("process"));
        push_shallow_dir(&mut out, "BBL/filament", &bbl.join("filament"));
    }
    out
}

pub(crate) fn load_cura_snapshot(
    cura: &Path,
    materials: &Path,
    resources: &Path,
) -> Result<Option<CuraSnapshot>, Error> {
    load_cura_snapshot_in(cura, materials, resources, &cache_dir())
}

fn load_cura_snapshot_in(
    cura: &Path,
    materials: &Path,
    resources: &Path,
    cache_root: &Path,
) -> Result<Option<CuraSnapshot>, Error> {
    let key = path_key(&[cura, materials]);
    let file = cache_root.join(format!("cura-extras-{key:x}.json"));
    if !file.is_file() {
        return Ok(None);
    }
    let reader = BufReader::new(
        fs::File::open(&file).map_err(|err| Error::message(err.to_string()))?,
    );
    let cached: CuraExtrasCache =
        serde_json::from_reader(reader).map_err(|err| Error::message(err.to_string()))?;
    if cached.version != CACHE_VERSION {
        return Ok(None);
    }
    let live = cura_markers(resources, materials);
    if cached.markers != live {
        return Ok(None);
    }
    if cached.definitions.is_empty() {
        return Ok(None);
    }
    let definitions = cached
        .definitions
        .into_iter()
        .map(|definition| (definition.id.clone(), definition))
        .collect();
    Ok(Some(CuraSnapshot {
        definitions,
        instances: cached.instances,
        materials: cached.materials,
    }))
}

/// Writes the packed snapshot (definitions, instances, materials) to disk.
pub(crate) fn store_cura_snapshot(
    cura: PathBuf,
    materials: PathBuf,
    resources: PathBuf,
    definitions: &HashMap<String, Definition>,
    instances: &[InstanceFile],
    material_rows: &[Material],
) {
    let definition_rows: Vec<Definition> = definitions.values().cloned().collect();
    let instances = instances.to_vec();
    let material_rows = material_rows.to_vec();
    if test_mode_active() {
        let cache_root = cache_dir();
        let _ = store_cura_snapshot_sync(
            &cura,
            &materials,
            &resources,
            &definition_rows,
            &instances,
            &material_rows,
            &cache_root,
        );
    } else {
        let cache_root = cache_dir();
        std::thread::spawn(move || {
            let _ = store_cura_snapshot_sync(
                &cura,
                &materials,
                &resources,
                &definition_rows,
                &instances,
                &material_rows,
                &cache_root,
            );
        });
    }
}

fn store_cura_snapshot_sync(
    cura: &Path,
    materials: &Path,
    resources: &Path,
    definition_rows: &[Definition],
    instances: &[InstanceFile],
    material_rows: &[Material],
    cache_root: &Path,
) -> Result<(), Error> {
    let key = path_key(&[cura, materials]);
    fs::create_dir_all(cache_root).map_err(|err| Error::message(err.to_string()))?;
    let body = CuraExtrasCache {
        version: CACHE_VERSION,
        markers: cura_markers(resources, materials),
        definitions: definition_rows.to_vec(),
        instances: instances.to_vec(),
        materials: material_rows.to_vec(),
    };
    let file = cache_root.join(format!("cura-extras-{key:x}.json"));
    let writer = BufWriter::new(
        fs::File::create(&file).map_err(|err| Error::message(err.to_string()))?,
    );
    serde_json::to_writer(writer, &body).map_err(|err| Error::message(err.to_string()))?;
    Ok(())
}

pub(crate) fn load_bambu_library(root: &Path) -> Option<Library> {
    let key = path_key(&[root]);
    let file = cache_dir().join(format!("bambu-library-{key:x}.json"));
    if !file.is_file() {
        return None;
    }
    let reader = BufReader::new(fs::File::open(&file).ok()?);
    let cached: BambuLibraryCache = serde_json::from_reader(reader).ok()?;
    if cached.version != BAMBU_CACHE_VERSION {
        return None;
    }
    let live = bambu_markers(root);
    if cached.markers != live {
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
    if test_mode_active() {
        let cache_root = cache_dir();
        let _ = store_bambu_library_sync(&root, &library, &cache_root);
    } else {
        let cache_root = cache_dir();
        std::thread::spawn(move || {
            let _ = store_bambu_library_sync(&root, &library, &cache_root);
        });
    }
}

fn store_bambu_library_sync(
    root: &Path,
    library: &Library,
    cache_root: &Path,
) -> Result<(), Error> {
    let key = path_key(&[root]);
    fs::create_dir_all(cache_root).map_err(|err| Error::message(err.to_string()))?;
    let body = BambuLibraryCache {
        version: BAMBU_CACHE_VERSION,
        markers: bambu_markers(root),
        definitions: library.definitions.clone(),
        instances: library.instances.clone(),
        materials: library.materials.clone(),
        gcode: library.gcode.clone(),
    };
    let json = serde_json::to_vec(&body).map_err(|err| Error::message(err.to_string()))?;
    let file = dir.join(format!("bambu-library-{key:x}.json"));
    fs::write(&file, json).map_err(|err| Error::message(err.to_string()))?;
    Ok(())
}

const RESOURCE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Used only by cache regression tests on the fixture tree (no Bambu install scan).
#[cfg(test)]
pub(crate) fn fingerprint_fixture(resources: &Path, materials: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    let stamp = cura_markers(resources, materials);
    for (label, sig) in stamp.entries {
        label.hash(&mut hasher);
        sig.mtime_secs.hash(&mut hasher);
        sig.size.hash(&mut hasher);
    }
    hash_stamp_tree(&mut hasher, materials, 2);
    hasher.finish()
}

#[cfg(test)]
fn hash_stamp_tree(hasher: &mut DefaultHasher, dir: &Path, depth: u32) {
    RESOURCE_VERSION.hash(hasher);
    dir.display().to_string().hash(hasher);
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut count = 0u64;
    let mut bytes = 0u64;
    let mut newest = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        count += 1;
        bytes = bytes.saturating_add(meta.len());
        if let Ok(modified) = meta.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                newest = newest.max(duration.as_secs());
            }
        }
        if path.is_dir() {
            hash_stamp_tree(hasher, &path, depth.saturating_sub(1));
        }
    }
    newest.hash(hasher);
    count.hash(hasher);
    bytes.hash(hasher);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct ThreadCacheGuard {
        _temp: TempDir,
        path: PathBuf,
    }

    impl ThreadCacheGuard {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = temp.path().to_path_buf();
            set_cache_dir_thread_local(path.clone());
            Self {
                _temp: temp,
                path,
            }
        }
    }

    impl Drop for ThreadCacheGuard {
        fn drop(&mut self) {
            clear_cache_dir_thread_local();
        }
    }

    #[test]
    fn cache_writes_under_thread_local_not_user_profile() {
        let guard = ThreadCacheGuard::new();
        let dir = cache_dir();
        assert_eq!(dir, guard.path);
    }

    #[test]
    fn packed_snapshot_includes_definitions() {
        let cache = tempfile::tempdir().expect("tempdir");
        let cache_root = cache.path();
        std::env::set_var("SHALE_TEST", "1");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cura = root.join("cura");
        let materials = root.join("materials");
        let resources = cura.join("resources");
        let mut definitions = std::collections::HashMap::new();
        let def_path = resources.join("definitions/fdmprinter.def.json");
        let definition =
            crate::definition::load_definition(&def_path, false).expect("fdmprinter");
        definitions.insert(definition.id.clone(), definition);
        let definition_rows: Vec<Definition> = definitions.values().cloned().collect();
        store_cura_snapshot_sync(
            &cura,
            &materials,
            &resources,
            &definition_rows,
            &[],
            &[],
            cache_root,
        )
        .expect("write snapshot");
        let snapshot = load_cura_snapshot_in(&cura, &materials, &resources, cache_root)
            .expect("read")
            .expect("hit");
        assert!(snapshot.definitions.contains_key("fdmprinter"));
    }

    #[test]
    fn fingerprint_changes_when_a_file_is_added() {
        let base = tempfile::tempdir().unwrap();
        let materials = base.path().join("materials");
        let resources = base.path().join("cura");
        fs::create_dir_all(resources.join("definitions")).unwrap();
        fs::create_dir_all(&materials).unwrap();
        fs::write(resources.join("definitions/fdmprinter.def.json"), b"{}").unwrap();
        let a = fingerprint_fixture(&resources, &materials);
        fs::write(resources.join("definitions/extra.def.json"), b"{}").unwrap();
        let b = fingerprint_fixture(&resources, &materials);
        assert_ne!(a, b);
    }
}
