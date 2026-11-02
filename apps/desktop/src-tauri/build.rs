fn main() {
    // `tauri::generate_context!` refuses to compile until the Vite output
    // directory exists. A placeholder lets `cargo test` run before the first
    // frontend build; `npm run build` replaces it.
    let dist = std::path::Path::new("../dist");
    if !dist.join("index.html").exists() {
        let _ = std::fs::create_dir_all(dist);
        let _ = std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>Shale</title><p>Run npm run build.</p>",
        );
    }
    tauri_build::build()
}
