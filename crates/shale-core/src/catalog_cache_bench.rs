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
    fn fixture_catalog_open_stays_fast() {
        let _dirs = bench_dirs();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let paths = ResourcePaths {
            cura: root.join("cura"),
            materials: root.join("materials"),
        };
        let start = Instant::now();
        let _ = Catalog::open(paths.clone()).expect("fixture catalog");
        let elapsed = start.elapsed();
        eprintln!("fixture Catalog::open {:?}", elapsed);
        assert!(elapsed.as_millis() < 600, "fixture open took {:?}", elapsed);
        let warm = Instant::now();
        let _ = Catalog::open(paths).expect("fixture catalog from snapshot");
        let warm_elapsed = warm.elapsed();
        eprintln!("fixture Catalog::open (snapshot) {:?}", warm_elapsed);
        assert!(
            warm_elapsed.as_millis() < 400,
            "snapshot reopen took {:?}",
            warm_elapsed
        );
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
