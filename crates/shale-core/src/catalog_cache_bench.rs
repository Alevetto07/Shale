//! Lightweight timing guard for catalogue cache regressions (fixture tree only).

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use crate::catalog::{Catalog, ResourcePaths};
    use crate::catalog_cache;

    #[test]
    fn fixture_catalog_open_stays_fast() {
        let dir = std::env::temp_dir().join(format!("shale-bench-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        std::env::set_var("SHALE_CACHE_DIR", &dir);

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let paths = ResourcePaths {
            cura: root.join("cura"),
            materials: root.join("materials"),
        };
        let start = Instant::now();
        let _ = Catalog::open(paths).expect("fixture catalog");
        let elapsed = start.elapsed();
        eprintln!("fixture Catalog::open {:?}", elapsed);
        assert!(
            elapsed.as_millis() < 600,
            "fixture open took {:?}; cache fingerprint should stay cheap",
            elapsed
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cheap_fingerprint_is_millisecond_scale() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let resources = root.join("cura");
        let start = Instant::now();
        for _ in 0..20 {
            let _ = catalog_cache::fingerprint_cura(&resources, &resources, &resources);
        }
        let elapsed = start.elapsed();
        eprintln!("20 fingerprints {:?}", elapsed);
        assert!(
            elapsed.as_millis() < 200,
            "fingerprinting took {:?} for 20 runs",
            elapsed
        );
    }
}