//! The window's side of MakerWorld and Thingiverse.
//!
//! Search and file bytes live in `shale_host::models`. This file only reads the
//! tokens the window already has: the Bambu Cloud sign-in, and a Thingiverse
//! app token the user pasted. The token is not returned to the page.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use shale_host::cloud::HttpTransport;
use shale_host::models::{self, Browser, Credentials, Library, Pace, SavedModel, SearchPage, Sort};
use tauri::State;

use crate::cloud::CloudState;

fn token_path() -> std::path::PathBuf {
    shale_core::config_dir().join("thingiverse-token")
}

fn thingiverse_token() -> Option<String> {
    if let Ok(Some(token)) = crate::secrets::get("thingiverse.token") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }
    models::read_token_file(&token_path()).ok().flatten()
}

fn browser(bambu: Option<String>) -> Browser<HttpTransport> {
    Browser::new(
        Arc::new(HttpTransport),
        models::cache_dir(),
        Credentials {
            thingiverse_token: thingiverse_token(),
            bambu_token: bambu,
        },
        Pace::shared(Duration::from_millis(400)),
    )
}

fn bambu_token(state: &CloudState) -> Option<String> {
    let Ok(mut cloud) = crate::cloud::lock(state) else {
        return None;
    };
    if !cloud.signed_in() {
        return None;
    }
    cloud.bearer().ok()
}

async fn run<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(result) => result,
        Err(err) => Err(format!("the model library stopped: {err}")),
    }
}

#[tauri::command]
pub async fn search_models(
    text: String,
    library: String,
    page: Option<u32>,
    sort: Option<String>,
) -> Result<SearchPage, String> {
    let library = Library::parse(&library).map_err(|err| err.to_string())?;
    let sort = Sort::parse(sort.as_deref().unwrap_or("relevant")).map_err(|err| err.to_string())?;
    let page = page.unwrap_or(1);
    run(move || {
        browser(None)
            .search(&text, library, page, sort)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn model_detail(library: String, id: String) -> Result<models::Listing, String> {
    let library = Library::parse(&library).map_err(|err| err.to_string())?;
    run(move || {
        browser(None)
            .detail(library, &id)
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn download_model(
    state: State<'_, CloudState>,
    library: String,
    id: String,
    profile_id: Option<String>,
    file_id: Option<String>,
) -> Result<SavedModel, String> {
    let library = Library::parse(&library).map_err(|err| err.to_string())?;
    let bambu = bambu_token(&state);
    run(move || {
        browser(bambu)
            .download(library, &id, profile_id.as_deref(), file_id.as_deref())
            .map_err(|err| err.to_string())
    })
    .await
}

#[tauri::command]
pub async fn import_model_url(
    state: State<'_, CloudState>,
    url: String,
    profile_id: Option<String>,
    file_id: Option<String>,
) -> Result<SavedModel, String> {
    let bambu = bambu_token(&state);
    run(move || {
        browser(bambu)
            .import_url(&url, profile_id.as_deref(), file_id.as_deref())
            .map_err(|err| err.to_string())
    })
    .await
}

#[derive(Serialize)]
pub struct TokenStatus {
    pub configured: bool,
}

#[tauri::command]
pub fn thingiverse_token_status() -> TokenStatus {
    TokenStatus {
        configured: thingiverse_token().is_some(),
    }
}

#[tauri::command]
pub fn thingiverse_token_set(token: String) -> Result<TokenStatus, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("paste the app token from the Thingiverse developer page".into());
    }
    if token.len() > 512 {
        return Err("that token is too long".into());
    }
    #[cfg(windows)]
    {
        crate::secrets::set("thingiverse.token", &token)?;
    }
    #[cfg(not(windows))]
    {
        models::write_token_file(&token_path(), &token)?;
    }
    Ok(TokenStatus { configured: true })
}

#[tauri::command]
pub fn thingiverse_token_clear() -> Result<TokenStatus, String> {
    let _ = crate::secrets::clear("thingiverse.token");
    models::delete_token_file(&token_path())?;
    Ok(TokenStatus { configured: false })
}

#[tauri::command]
pub fn open_model_link(url: String) -> Result<(), String> {
    models::openable_listing(&url).map_err(|err| err.to_string())?;
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|err| format!("could not open the listing: {err}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|err| format!("could not open the listing: {err}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|err| format!("could not open the listing: {err}"))?;
    }
    Ok(())
}
