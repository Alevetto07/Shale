//! Lightweight timing guard for catalogue cache regressions (fixture tree only).

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::time::Instant;

    use crate::bambu;
    use crate::catalog::{Catalog, ResourcePaths};
    use crate::catalog_cache;

    struct BenchDirs {
        _orca: tempfile::TempDir,
        _cache: tempfile::TempDir,
    }

    fn bench_dirs() -> &'static BenchDirs {
        static DIRS: OnceLock<BenchDirs> = OnceLock::new();
        DIRS.get_or_init(|| {
            std::env::set_var("SHALE_TEST", "1");
            let orca = tempfile::tempdir().expect("bench orca tempdir");
            bambu::set_test_profiles_root(Some(orca.path().to_path_buf()));
            let cache = tempfile::tempdir().expect("bench cache tempdir");
            catalog_cache::set_cache_dir_override(cache.path().to_path_buf());
            BenchDirs {
                _orca: orca,
                _cache: cache,
            }
        })
    }

    #[test]
    #[cfg(unix)]
    fn packed_snapshot_reopens_when_source_trees_are_unreadable() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let _dirs = bench_dirs();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let paths = ResourcePaths {
            cura: root.join("cura"),
            materials: root.join("materials"),
        };
        let resources = paths.resources();
        let _ = Catalog::open(paths.clone()).expect("seed on-disk snapshot");

        for dir in [
            resources.join("quality"),
            resources.join("variants"),
            resources.join("definitions"),
            paths.materials.clone(),
        ] {
            if dir.is_dir() {
                let mut perms = fs::metadata(&dir).expect("metadata").permissions();
                perms.set_mode(0);
                let _ = fs::set_permissions(&dir, perms);
            }
        }

        let start = Instant::now();
        let catalog = Catalog::open(paths).expect("snapshot should satisfy open");
        let elapsed = start.elapsed();
        eprintln!("snapshot Catalog::open {:?}", elapsed);
        assert!(catalog.definitions.contains_key("fdmprinter"));
        assert!(
            elapsed.as_secs() < 3,
            "packed snapshot cold path took {:?}",
            elapsed
        );
    }

    #[test]
    fn fixture_catalog_open_stays_fast() {
        let _dirs = bench_dirs();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let paths = ResourcePaths {
            cura: root.join("cura"),
            materials: root.join("materials"),
        };
        let start = Instant::now();
        let _ = Catalog::open(paths).expect("fixture catalog");
        let elapsed = start.elapsed();
        eprintln!("fixture Catalog::open {:?}", elapsed);
        assert!(elapsed.as_millis() < 600, "fixture open took {:?}", elapsed);
    }

    #[test]
    fn cheap_fingerprint_is_millisecond_scale() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let resources = root.join("cura");
        let materials = root.join("materials");
        let start = Instant::now();
        for _ in 0..20 {
            let _ = catalog_cache::fingerprint_fixture(&resources, &materials);
        }
        let elapsed = start.elapsed();
        eprintln!("20 fixture fingerprints {:?}", elapsed);
        assert!(
            elapsed.as_millis() < 200,
            "fingerprinting took {:?} for 20 runs",
            elapsed
        );
    }
}
