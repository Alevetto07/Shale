//! Search and download for MakerWorld and Thingiverse.
//!
//! MakerWorld has no public API document. Bambu Studio opens the site in a
//! web view and fetches files through a closed plugin. The calls in
//! [`makerworld`] are the ones that plugin and the MakerWorld site make on
//! `api.bambulab.com`: search needs no account, and a 3MF is a short-lived
//! link minted with the Bambu Cloud token Shale already stores. Nothing here
//! asks for a password.
//!
//! Thingiverse's REST API is official and needs an app token the user creates.
//! There is no token in the source.
//!
//! JSON answers are cached under the model-browser directory inside
//! `SHALE_CACHE_DIR` (or Shale's own cache directory). Tests pass their own
//! directory. The catalogue cache is never used. Outbound calls are paced.
//! Signed download links are not cached: they expire in a few minutes.

mod makerworld;
mod thingiverse;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cloud::http::{self, Response, Url};
use crate::cloud::{HttpRequest, HttpTransport};

const JSON_CAP: usize = 2 * 1024 * 1024;
const FILE_CAP: usize = 200 * 1024 * 1024;
const PAGE_CAP: u32 = 50;

/// Which library a card came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Library {
    MakerWorld,
    Thingiverse,
}

impl Library {
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        match text.trim().to_ascii_lowercase().as_str() {
            "makerworld" | "maker-world" => Ok(Self::MakerWorld),
            "thingiverse" => Ok(Self::Thingiverse),
            other => Err(ModelError::Refused(format!(
                "library '{other}' is not makerworld or thingiverse"
            ))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::MakerWorld => "MakerWorld",
            Self::Thingiverse => "Thingiverse",
        }
    }
}

/// How to order a page of results.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    Relevant,
    Popular,
    Downloads,
    Newest,
}

impl Sort {
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "relevant" | "relevance" => Ok(Self::Relevant),
            "popular" | "likes" => Ok(Self::Popular),
            "downloads" | "download" => Ok(Self::Downloads),
            "newest" | "new" => Ok(Self::Newest),
            other => Err(ModelError::Refused(format!(
                "sort '{other}' is not relevant, popular, downloads, or newest"
            ))),
        }
    }
}

/// A card in a search result. Creator and licence are always filled in.
#[derive(Clone, Debug, Serialize)]
pub struct ModelCard {
    pub library: Library,
    pub id: String,
    pub title: String,
    pub creator: String,
    pub creator_url: Option<String>,
    pub licence: String,
    pub thumbnail_url: Option<String>,
    pub page_url: String,
    pub likes: Option<u64>,
    pub downloads: Option<u64>,
    pub nsfw: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchPage {
    pub total: u64,
    pub hits: Vec<ModelCard>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PrintProfile {
    pub id: String,
    pub name: String,
    pub printer: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LibraryFile {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Listing {
    pub card: ModelCard,
    pub summary: String,
    pub profiles: Vec<PrintProfile>,
    pub files: Vec<LibraryFile>,
    /// MakerWorld's alphanumeric model id. Empty for Thingiverse.
    pub remote_model_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SavedModel {
    pub path: String,
    pub file_name: String,
    pub title: String,
    pub creator: String,
    pub licence: String,
    pub page_url: String,
    pub library: Library,
}

/// A model page the user pasted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelLink {
    pub library: Library,
    pub id: String,
    pub profile_id: Option<String>,
    pub file_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Credentials {
    pub thingiverse_token: Option<String>,
    pub bambu_token: Option<String>,
}

#[derive(Debug)]
pub enum ModelError {
    NeedsThingiverseToken,
    NeedsBambuSignIn,
    Blocked,
    Remote {
        call: String,
        status: u16,
        message: String,
    },
    Unreachable {
        call: String,
        message: String,
    },
    Shape {
        call: String,
        message: String,
    },
    Refused(String),
    Io(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedsThingiverseToken => f.write_str(
                "Thingiverse needs an app token. Add one in Settings. Create a desktop app on the Thingiverse developer page: https://www.thingiverse.com/developers",
            ),
            Self::NeedsBambuSignIn => f.write_str(
                "Sign in with your Bambu Lab account to download this model.",
            ),
            Self::Blocked => f.write_str(
                "MakerWorld did not return any models for that search. Try again in a moment.",
            ),
            Self::Remote { call, status, message } => {
                if message.is_empty() || message.starts_with('{') {
                    write!(f, "{call} was refused ({status}).")
                } else {
                    write!(f, "{call} was refused ({status}): {message}")
                }
            }
            Self::Unreachable { call, message } => {
                write!(f, "{call} could not be reached: {message}")
            }
            Self::Shape { call, message } => {
                write!(f, "{call} answered something unexpected: {message}")
            }
            Self::Refused(message) => f.write_str(message),
            Self::Io(message) => write!(f, "could not store the download: {message}"),
        }
    }
}

impl std::error::Error for ModelError {}

/// Where model-browser cache files go. Never the catalogue cache.
pub struct CachePlaces<'a> {
    pub shale: Option<&'a str>,
    pub test: bool,
    pub appdata: Option<&'a str>,
    pub home: Option<&'a str>,
    pub pid: u32,
}

pub fn cache_dir_from(places: &CachePlaces<'_>) -> PathBuf {
    if let Some(dir) = places.shale.map(str::trim).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir).join("model-browser");
    }
    if places.test {
        return std::env::temp_dir().join(format!("shale-model-browser-test-{}", places.pid));
    }
    if let Some(roaming) = places.appdata.map(str::trim).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(roaming).join("Shale").join("model-browser");
    }
    if let Some(home) = places.home.map(str::trim).filter(|dir| !dir.is_empty()) {
        return PathBuf::from(home)
            .join(".cache")
            .join("shale")
            .join("model-browser");
    }
    std::env::temp_dir().join(format!("shale-model-browser-{}", places.pid))
}

pub fn cache_dir() -> PathBuf {
    cache_dir_from(&CachePlaces {
        shale: std::env::var("SHALE_CACHE_DIR").ok().as_deref(),
        test: cfg!(test),
        appdata: std::env::var("APPDATA").ok().as_deref(),
        home: std::env::var("HOME").ok().as_deref(),
        pid: std::process::id(),
    })
}

pub fn read_token_file(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(text))
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

pub fn write_token_file(path: &Path, token: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|err| err.to_string())?;
    file.write_all(token.trim().as_bytes())
        .map_err(|err| err.to_string())?;
    Ok(())
}

pub fn delete_token_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// How long to wait since the previous outbound call.
pub(crate) fn gap_remaining(last: Option<Instant>, now: Instant, gap: Duration) -> Duration {
    match last {
        Some(prev) => gap.saturating_sub(now.saturating_duration_since(prev)),
        None => Duration::ZERO,
    }
}

/// Shared by every browser the process builds with [`Pace::shared`].
#[derive(Clone)]
pub struct Pace {
    gap: Duration,
    last: Arc<Mutex<Option<Instant>>>,
}

impl Pace {
    pub fn none() -> Self {
        Self {
            gap: Duration::ZERO,
            last: Arc::new(Mutex::new(None)),
        }
    }

    pub fn shared(gap: Duration) -> Self {
        static LAST: OnceLock<Arc<Mutex<Option<Instant>>>> = OnceLock::new();
        Self {
            gap,
            last: LAST.get_or_init(|| Arc::new(Mutex::new(None))).clone(),
        }
    }

    pub fn wait(&self) {
        let pause = {
            let mut last = self.last.lock().unwrap_or_else(|err| err.into_inner());
            let now = Instant::now();
            let pause = gap_remaining(*last, now, self.gap);
            *last = Some(now + pause);
            pause
        };
        if !pause.is_zero() {
            std::thread::sleep(pause);
        }
    }
}

pub trait Fetch: Send + Sync {
    fn fetch(&self, request: HttpRequest, max_bytes: usize) -> Result<Response, String>;
}

impl Fetch for HttpTransport {
    fn fetch(&self, request: HttpRequest, max_bytes: usize) -> Result<Response, String> {
        let headers: Vec<(&str, String)> = request
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone()))
            .collect();
        http::send_limited(
            request.method,
            &request.url,
            &headers,
            request.body,
            Some(max_bytes),
        )
        .map_err(|err| err.to_string())
    }
}

pub struct Browser<T: Fetch> {
    transport: Arc<T>,
    cache: PathBuf,
    credentials: Credentials,
    pace: Pace,
    ttl_secs: i64,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl<T: Fetch> std::fmt::Debug for Browser<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Browser")
            .field("cache", &self.cache)
            .field(
                "thingiverse_token",
                &self.credentials.thingiverse_token.as_ref().map(|_| "set"),
            )
            .field(
                "bambu_token",
                &self.credentials.bambu_token.as_ref().map(|_| "set"),
            )
            .finish()
    }
}

impl<T: Fetch> Browser<T> {
    pub fn new(transport: Arc<T>, cache: PathBuf, credentials: Credentials, pace: Pace) -> Self {
        Self {
            transport,
            cache,
            credentials,
            pace,
            ttl_secs: 15 * 60,
            now: Arc::new(crate::cloud::now_unix),
        }
    }

    pub fn search(
        &self,
        text: &str,
        library: Library,
        page: u32,
        sort: Sort,
    ) -> Result<SearchPage, ModelError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ModelError::Refused("say what to search for".into()));
        }
        if text.chars().count() > 120 {
            return Err(ModelError::Refused("that search is too long".into()));
        }
        let page = page.clamp(1, PAGE_CAP);
        match library {
            Library::MakerWorld => {
                let url = makerworld::search_url(text, sort, page);
                let value = self.get_json("MakerWorld search", &url, None, true)?;
                makerworld::parse_search(&value)
            }
            Library::Thingiverse => {
                let token = self.thingiverse_token()?;
                let url = thingiverse::search_url(text, sort, page);
                let value = self.get_json("Thingiverse search", &url, Some(token), true)?;
                thingiverse::parse_search(&value)
            }
        }
    }

    pub fn detail(&self, library: Library, id: &str) -> Result<Listing, ModelError> {
        match library {
            Library::MakerWorld => self.makerworld_listing(numeric_id(id, "MakerWorld")?),
            Library::Thingiverse => self.thingiverse_listing(numeric_id(id, "Thingiverse")?),
        }
    }

    pub fn download(
        &self,
        library: Library,
        id: &str,
        profile_id: Option<&str>,
        file_id: Option<&str>,
    ) -> Result<SavedModel, ModelError> {
        match library {
            Library::MakerWorld => {
                self.download_makerworld(numeric_id(id, "MakerWorld")?, profile_id)
            }
            Library::Thingiverse => {
                self.download_thingiverse(numeric_id(id, "Thingiverse")?, file_id)
            }
        }
    }

    pub fn import_url(
        &self,
        url: &str,
        profile_id: Option<&str>,
        file_id: Option<&str>,
    ) -> Result<SavedModel, ModelError> {
        let link = parse_model_url(url)?;
        let profile = profile_id.or(link.profile_id.as_deref());
        let file = file_id.or(link.file_id.as_deref());
        self.download(link.library, &link.id, profile, file)
    }

    fn makerworld_listing(&self, id: &str) -> Result<Listing, ModelError> {
        let value = self.get_json("MakerWorld model", &makerworld::design_url(id), None, true)?;
        let mut listing = makerworld::parse_listing(&value)?;
        if listing.profiles.is_empty() {
            let extra = self.get_json(
                "MakerWorld print profiles",
                &makerworld::instances_url(id),
                None,
                true,
            )?;
            listing.profiles = makerworld::parse_instances(&extra);
        }
        Ok(listing)
    }

    fn thingiverse_listing(&self, id: &str) -> Result<Listing, ModelError> {
        let token = self.thingiverse_token()?;
        let thing = self.get_json(
            "Thingiverse model",
            &thingiverse::thing_url(id),
            Some(token),
            true,
        )?;
        let mut listing = thingiverse::parse_thing(&thing)?;
        let files = self.get_json(
            "Thingiverse files",
            &thingiverse::files_url(id),
            Some(token),
            true,
        )?;
        listing.files = thingiverse::parse_files(&files)
            .into_iter()
            .map(|file| file.file)
            .collect();
        Ok(listing)
    }

    fn download_makerworld(
        &self,
        id: &str,
        profile_id: Option<&str>,
    ) -> Result<SavedModel, ModelError> {
        let listing = self.makerworld_listing(id)?;
        if listing.remote_model_id.is_empty() {
            return Err(ModelError::Shape {
                call: "MakerWorld model".into(),
                message: "the listing had no model id, so the file cannot be requested".into(),
            });
        }
        let profile = pick_profile(&listing.profiles, profile_id)?;
        let token = self.bambu_token()?;
        let signed = self.get_json(
            "MakerWorld download",
            &makerworld::profile_url(&profile.id, &listing.remote_model_id),
            Some(token),
            false,
        )?;
        if signed.get("code").and_then(Value::as_i64) == Some(4) {
            return Err(ModelError::NeedsBambuSignIn);
        }
        let (url, name) = makerworld::signed_download(&signed)?;
        let bytes = self.get_bytes("MakerWorld file", &url, None, 0)?;
        let file_name = safe_file_name(&name, "3mf");
        self.store(&listing.card, &file_name, &bytes)
    }

    fn download_thingiverse(
        &self,
        id: &str,
        file_id: Option<&str>,
    ) -> Result<SavedModel, ModelError> {
        let token = self.thingiverse_token()?;
        let thing = self.get_json(
            "Thingiverse model",
            &thingiverse::thing_url(id),
            Some(token),
            true,
        )?;
        let listing = thingiverse::parse_thing(&thing)?;
        let files = self.get_json(
            "Thingiverse files",
            &thingiverse::files_url(id),
            Some(token),
            true,
        )?;
        let files = thingiverse::parse_files(&files);
        let chosen = pick_remote(&files, file_id)?;
        let auth = if chosen.url.contains("api.thingiverse.com") {
            Some(token)
        } else {
            None
        };
        let bytes = self.get_bytes("Thingiverse file", &chosen.url, auth, 0)?;
        self.store(
            &listing.card,
            &safe_file_name(&chosen.file.name, &chosen.file.kind),
            &bytes,
        )
    }

    fn bambu_token(&self) -> Result<&str, ModelError> {
        self.credentials
            .bambu_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or(ModelError::NeedsBambuSignIn)
    }

    fn thingiverse_token(&self) -> Result<&str, ModelError> {
        self.credentials
            .thingiverse_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or(ModelError::NeedsThingiverseToken)
    }

    fn get_json(
        &self,
        call: &str,
        url: &str,
        auth: Option<&str>,
        cacheable: bool,
    ) -> Result<Value, ModelError> {
        check_url(url)?;
        if cacheable {
            if let Some(bytes) = read_cache(&self.cache, url, (self.now)()) {
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    return Ok(value);
                }
            }
        }
        let response = self.send(call, url, auth, JSON_CAP)?;
        if response.status == 401 || response.status == 403 {
            if call.contains("MakerWorld download") {
                return Err(ModelError::NeedsBambuSignIn);
            }
            if call.contains("Thingiverse") {
                return Err(ModelError::NeedsThingiverseToken);
            }
        }
        if !(200..300).contains(&response.status) {
            return Err(self.remote(call, &response));
        }
        let value = response.json().map_err(|_| ModelError::Shape {
            call: call.into(),
            message: "the answer was not JSON".into(),
        })?;
        if cacheable {
            let _ = write_cache(
                &self.cache,
                url,
                &response.body,
                (self.now)(),
                self.ttl_secs,
            );
        }
        Ok(value)
    }

    fn get_bytes(
        &self,
        call: &str,
        url: &str,
        auth: Option<&str>,
        hops: u8,
    ) -> Result<Vec<u8>, ModelError> {
        if hops > 3 {
            return Err(ModelError::Refused(
                "the download redirected too many times".into(),
            ));
        }
        check_url(url)?;
        let response = self.send(call, url, auth, FILE_CAP)?;
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            let location = response
                .header("location")
                .ok_or_else(|| ModelError::Shape {
                    call: call.into(),
                    message: "a redirect came back without a destination".into(),
                })?;
            if location.len() > 4096 {
                return Err(ModelError::Refused("the download link was refused".into()));
            }
            let next = resolve_redirect(url, location)?;
            let auth = if same_host(url, &next) { auth } else { None };
            return self.get_bytes(call, &next, auth, hops + 1);
        }
        if !(200..300).contains(&response.status) {
            return Err(self.remote(call, &response));
        }
        Ok(response.body)
    }

    fn send(
        &self,
        call: &str,
        url: &str,
        auth: Option<&str>,
        max_bytes: usize,
    ) -> Result<Response, ModelError> {
        self.pace.wait();
        let mut headers = Vec::new();
        if let Some(token) = auth {
            headers.push(("Authorization".into(), format!("Bearer {token}")));
        }
        self.transport
            .fetch(
                HttpRequest {
                    name: "model",
                    method: "GET",
                    url: url.to_string(),
                    headers,
                    body: None,
                },
                max_bytes,
            )
            .map_err(|message| ModelError::Unreachable {
                call: call.into(),
                message: redact(&message, &self.credentials),
            })
    }

    fn remote(&self, call: &str, response: &Response) -> ModelError {
        let message = String::from_utf8_lossy(&response.body)
            .chars()
            .take(180)
            .collect::<String>();
        ModelError::Remote {
            call: call.into(),
            status: response.status,
            message: redact(&message, &self.credentials),
        }
    }

    fn store(
        &self,
        card: &ModelCard,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<SavedModel, ModelError> {
        if !acceptable(bytes, file_name) {
            return Err(ModelError::Refused(
                "the download was not an STL or 3MF".into(),
            ));
        }
        let dir = self.cache.join("files").join(format!(
            "{}-{}",
            match card.library {
                Library::MakerWorld => "makerworld",
                Library::Thingiverse => "thingiverse",
            },
            card.id
        ));
        fs::create_dir_all(&dir).map_err(|err| ModelError::Io(err.to_string()))?;
        let path = dir.join(file_name);
        fs::write(&path, bytes).map_err(|err| ModelError::Io(err.to_string()))?;
        let note = format!(
            "Title: {}\nCreator: {}\nLicence: {}\nSource: {}\nLibrary: {}\n",
            card.title,
            card.creator,
            card.licence,
            card.page_url,
            card.library.label()
        );
        let _ = fs::write(dir.join("attribution.txt"), note);
        Ok(SavedModel {
            path: path.to_string_lossy().into_owned(),
            file_name: file_name.to_string(),
            title: card.title.clone(),
            creator: card.creator.clone(),
            licence: card.licence.clone(),
            page_url: card.page_url.clone(),
            library: card.library,
        })
    }
}

pub fn parse_model_url(input: &str) -> Result<ModelLink, ModelError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(ModelError::Refused(
            "paste a MakerWorld or Thingiverse link".into(),
        ));
    }
    if let Some(rest) = text
        .strip_prefix("thing:")
        .or_else(|| text.strip_prefix("Thing:"))
    {
        return Ok(ModelLink {
            library: Library::Thingiverse,
            id: numeric_id(rest, "Thingiverse")?.to_string(),
            profile_id: None,
            file_id: None,
        });
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("javascript:") || lower.starts_with("file:") || lower.starts_with("data:")
    {
        return Err(ModelError::Refused("that link is not a model page".into()));
    }
    let hay = if lower.starts_with("http://") || lower.starts_with("https://") {
        text.to_string()
    } else if lower.contains("makerworld.com") || lower.contains("thingiverse.com") {
        format!("https://{text}")
    } else {
        return Err(ModelError::Refused(
            "that link is not a MakerWorld or Thingiverse model".into(),
        ));
    };
    let lower = hay.to_ascii_lowercase();
    if lower.contains("thingiverse.com") || lower.contains("thing:") {
        let id = digits_after(&lower, "thing:")
            .ok_or_else(|| ModelError::Refused("that Thingiverse link has no thing id".into()))?;
        return Ok(ModelLink {
            library: Library::Thingiverse,
            id,
            profile_id: None,
            file_id: digits_after(&lower, "files/"),
        });
    }
    if lower.contains("makerworld.com") {
        let id = digits_after(&lower, "models/")
            .ok_or_else(|| ModelError::Refused("that MakerWorld link has no model id".into()))?;
        let profile_id = digits_after(&lower, "profileid-")
            .or_else(|| query_value(&hay, "profileid"))
            .or_else(|| query_value(&hay, "profile"));
        return Ok(ModelLink {
            library: Library::MakerWorld,
            id,
            profile_id,
            file_id: None,
        });
    }
    Err(ModelError::Refused(
        "that link is not a MakerWorld or Thingiverse model".into(),
    ))
}

fn pick_profile(
    profiles: &[PrintProfile],
    wanted: Option<&str>,
) -> Result<PrintProfile, ModelError> {
    if let Some(wanted) = wanted.map(str::trim).filter(|id| !id.is_empty()) {
        return profiles
            .iter()
            .find(|profile| profile.id == wanted)
            .cloned()
            .ok_or_else(|| {
                ModelError::Refused(format!("this model has no print profile {wanted}"))
            });
    }
    profiles
        .first()
        .cloned()
        .ok_or_else(|| ModelError::Refused("this model has no print profile to download".into()))
}

fn pick_remote<'a>(
    files: &'a [thingiverse::RemoteFile],
    wanted: Option<&str>,
) -> Result<&'a thingiverse::RemoteFile, ModelError> {
    if let Some(wanted) = wanted.map(str::trim).filter(|id| !id.is_empty()) {
        return files
            .iter()
            .find(|file| file.file.id == wanted)
            .ok_or_else(|| ModelError::Refused(format!("this model has no file {wanted}")));
    }
    files
        .iter()
        .find(|file| file.file.kind == "3mf")
        .or_else(|| files.iter().find(|file| file.file.kind == "stl"))
        .ok_or_else(|| ModelError::Refused("this model has no STL or 3MF to download".into()))
}

fn numeric_id<'a>(id: &'a str, library: &str) -> Result<&'a str, ModelError> {
    let id = id.trim();
    if id.is_empty() || id.len() > 20 || !id.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(ModelError::Refused(format!(
            "that {library} id is not a number"
        )));
    }
    Ok(id)
}

fn digits_after(hay: &str, marker: &str) -> Option<String> {
    let index = hay.find(marker)?;
    let rest = &hay[index + marker.len()..];
    let id: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

fn query_value(url: &str, key: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    let query = query.split('#').next().unwrap_or(query);
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=')?;
        if name.eq_ignore_ascii_case(key) {
            let id: String = value.chars().take_while(|ch| ch.is_ascii_digit()).collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

fn encode_query(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn id_string(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        let text = text.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    value.as_u64().map(|number| number.to_string()).or_else(|| {
        value.as_i64().and_then(|number| {
            if number >= 0 {
                Some(number.to_string())
            } else {
                None
            }
        })
    })
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

pub(crate) fn kind_of(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".stl") {
        Some("stl".into())
    } else if lower.ends_with(".3mf") {
        Some("3mf".into())
    } else {
        None
    }
}

fn licence_label(code: &str) -> String {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return "Not stated".into();
    }
    let key = trimmed.to_ascii_lowercase().replace(['_', ' '], "-");
    let known = match key.as_str() {
        "cc0" | "cc-0" | "pd0" | "public-domain" => "CC0 1.0",
        "by" | "cc-by" | "cc" => "Creative Commons Attribution 4.0",
        "by-sa" | "cc-by-sa" | "cc-sa" => "Creative Commons Attribution-ShareAlike 4.0",
        "by-nc" | "cc-by-nc" | "cc-nc" => "Creative Commons Attribution-NonCommercial 4.0",
        "by-nd" | "cc-by-nd" | "cc-nd" => "Creative Commons Attribution-NoDerivatives 4.0",
        "by-nc-sa" | "cc-by-nc-sa" | "cc-nc-sa" => {
            "Creative Commons Attribution-NonCommercial-ShareAlike 4.0"
        }
        "by-nc-nd" | "cc-by-nc-nd" | "cc-nc-nd" => {
            "Creative Commons Attribution-NonCommercial-NoDerivatives 4.0"
        }
        "gpl" | "gpl-3.0" | "gpl-3" => "GNU GPL 3.0",
        "lgpl" | "lgpl-3.0" => "GNU LGPL 3.0",
        "bsd" => "BSD",
        "sdfl" | "standard-digital-file-license" | "standard-digital-file-licence" => {
            "Standard Digital File Licence"
        }
        "all-rights-reserved" => "All rights reserved",
        _ => return trimmed.to_string(),
    };
    known.to_string()
}

fn plain_text(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 600 {
        let end = collapsed
            .char_indices()
            .nth(600)
            .map(|(index, _)| index)
            .unwrap_or(collapsed.len());
        format!("{}…", collapsed[..end].trim_end())
    } else {
        collapsed
    }
}

fn safe_file_name(name: &str, fallback_ext: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut out = String::new();
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    while out.contains("..") {
        out = out.replace("..", ".");
    }
    let mut out = out.trim_matches('.').to_string();
    if out.is_empty() {
        out = format!("model.{fallback_ext}");
    }
    let lower = out.to_ascii_lowercase();
    if !lower.ends_with(".stl") && !lower.ends_with(".3mf") {
        out.push('.');
        out.push_str(fallback_ext);
    }
    if out.len() > 80 {
        let ext = if out.to_ascii_lowercase().ends_with(".3mf") {
            "3mf"
        } else {
            "stl"
        };
        out = format!("model.{ext}");
    }
    out
}

fn acceptable(bytes: &[u8], name: &str) -> bool {
    if bytes.len() < 16 {
        return false;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]).to_ascii_lowercase();
    if head.contains("<html") || head.contains("<!doctype") {
        return false;
    }
    let name = name.to_ascii_lowercase();
    if name.ends_with(".3mf") {
        return bytes.starts_with(b"PK");
    }
    name.ends_with(".stl") && bytes.len() > 84
}

/// A listing page the window may open in the user's browser.
pub fn openable_listing(url: &str) -> Result<(), ModelError> {
    let parsed = Url::parse(url).map_err(|err| ModelError::Refused(err.to_string()))?;
    if !parsed.secure {
        return Err(ModelError::Refused(
            "that listing is not an https page".into(),
        ));
    }
    let host = parsed.host.to_ascii_lowercase();
    let allowed = matches!(
        host.as_str(),
        "makerworld.com" | "www.makerworld.com" | "thingiverse.com" | "www.thingiverse.com"
    );
    if !allowed {
        return Err(ModelError::Refused(
            "that page is not a MakerWorld or Thingiverse listing".into(),
        ));
    }
    Ok(())
}

fn check_url(url: &str) -> Result<Url, ModelError> {
    let parsed = Url::parse(url).map_err(|err| ModelError::Refused(err.to_string()))?;
    let local = parsed.host == "127.0.0.1" || parsed.host.eq_ignore_ascii_case("localhost");
    if !parsed.secure && !local {
        return Err(ModelError::Refused(format!(
            "{} is not fetched over plain HTTP",
            parsed.host
        )));
    }
    if !host_allowed(&parsed.host) {
        return Err(ModelError::Refused(format!(
            "{} is not a model host Shale will fetch",
            parsed.host
        )));
    }
    Ok(parsed)
}

fn host_allowed(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "127.0.0.1" || host == "localhost" {
        return true;
    }
    const SUFFIXES: &[&str] = &[
        "bambulab.com",
        "makerworld.com",
        "thingiverse.com",
        "amazonaws.com",
        "cloudfront.net",
        "bblmw.com",
    ];
    SUFFIXES
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

fn same_host(left: &str, right: &str) -> bool {
    match (Url::parse(left), Url::parse(right)) {
        (Ok(left), Ok(right)) => left.host.eq_ignore_ascii_case(&right.host),
        _ => false,
    }
}

fn resolve_redirect(base: &str, location: &str) -> Result<String, ModelError> {
    let location = location.trim();
    if location.starts_with("https://") || location.starts_with("http://") {
        return Ok(location.to_string());
    }
    let parsed = Url::parse(base).map_err(|err| ModelError::Refused(err.to_string()))?;
    let scheme = if parsed.secure { "https" } else { "http" };
    let origin = if (parsed.secure && parsed.port == 443) || (!parsed.secure && parsed.port == 80) {
        format!("{scheme}://{}", parsed.host)
    } else {
        format!("{scheme}://{}:{}", parsed.host, parsed.port)
    };
    if let Some(rest) = location.strip_prefix('/') {
        return Ok(format!("{origin}/{rest}"));
    }
    Err(ModelError::Refused(
        "the download redirect was refused".into(),
    ))
}

fn redact(message: &str, credentials: &Credentials) -> String {
    let mut message = message.to_string();
    for token in [
        credentials.bambu_token.as_deref(),
        credentials.thingiverse_token.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if token.len() >= 8 {
            message = message.replace(token, "[redacted]");
        }
    }
    message
}

fn cache_key(url: &str) -> String {
    let digest = Sha256::digest(url.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_cache(dir: &Path, url: &str, now: i64) -> Option<Vec<u8>> {
    let bytes = fs::read(dir.join("http").join(cache_key(url))).ok()?;
    let split = bytes.iter().position(|byte| *byte == b'\n')?;
    let expiry: i64 = std::str::from_utf8(&bytes[..split]).ok()?.parse().ok()?;
    if now >= expiry {
        return None;
    }
    Some(bytes[split + 1..].to_vec())
}

fn write_cache(dir: &Path, url: &str, body: &[u8], now: i64, ttl_secs: i64) -> Result<(), ()> {
    let folder = dir.join("http");
    fs::create_dir_all(&folder).map_err(|_| ())?;
    let mut bytes = format!("{}\n", now.saturating_add(ttl_secs)).into_bytes();
    bytes.extend_from_slice(body);
    fs::write(folder.join(cache_key(url)), bytes).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const TINY_STL: &str = "solid clip\n  facet normal 0 0 1\n    outer loop\n      vertex 0 0 0\n      vertex 1 0 0\n      vertex 1 1 0\n    endloop\n  endfacet\nendsolid clip\n";
    const TOKEN: &str = "fixture-token-0123456789";

    struct Script {
        answers: Mutex<Vec<Response>>,
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl Script {
        fn new(answers: Vec<Response>) -> Arc<Self> {
            Arc::new(Self {
                answers: Mutex::new(answers),
                seen: Mutex::new(Vec::new()),
            })
        }

        fn seen(&self) -> Vec<HttpRequest> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl Fetch for Script {
        fn fetch(&self, request: HttpRequest, _max_bytes: usize) -> Result<Response, String> {
            self.seen.lock().unwrap().push(request);
            self.answers
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| "the fixture script has no answer left".into())
        }
    }

    fn json_answer(body: &str) -> Response {
        Response::from_parts(200, body.as_bytes().to_vec())
    }

    fn browser(script: Arc<Script>, credentials: Credentials) -> Browser<Script> {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "shale-model-browser-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        Browser::new(script, dir, credentials, Pace::none())
    }

    /// `pop` returns the last item, so the first body is pushed last.
    fn answers(bodies: &[&str]) -> Arc<Script> {
        Script::new(bodies.iter().rev().copied().map(json_answer).collect())
    }

    #[test]
    fn the_cache_directory_is_not_the_catalogue() {
        let dir = cache_dir_from(&CachePlaces {
            shale: None,
            test: true,
            appdata: Some(r"C:\Users\Alessio\AppData\Roaming"),
            home: Some("/home/alessio"),
            pid: 7,
        });
        let text = dir.to_string_lossy().to_ascii_lowercase();
        assert!(!text.contains("catalog-cache"), "{text}");
        assert!(text.contains("shale-model-browser"), "{text}");

        let explicit = cache_dir_from(&CachePlaces {
            shale: Some("/tmp/shale-cache"),
            test: false,
            appdata: Some(r"C:\Users\Alessio\AppData\Roaming"),
            home: None,
            pid: 1,
        });
        assert_eq!(
            explicit,
            PathBuf::from("/tmp/shale-cache").join("model-browser")
        );
        assert!(!explicit.to_string_lossy().contains("catalog-cache"));
    }

    #[test]
    fn a_gap_is_what_is_left_of_the_pause() {
        let now = Instant::now();
        assert_eq!(
            gap_remaining(None, now, Duration::from_millis(400)),
            Duration::ZERO
        );
        assert_eq!(
            gap_remaining(Some(now), now, Duration::from_millis(400)),
            Duration::from_millis(400)
        );
        assert_eq!(
            gap_remaining(
                Some(now - Duration::from_millis(500)),
                now,
                Duration::from_millis(400)
            ),
            Duration::ZERO
        );
    }

    #[test]
    fn model_links_keep_the_id_and_the_profile() {
        let maker =
            parse_model_url("https://makerworld.com/en/models/1400373-cable-clip#profileId-4242")
                .unwrap();
        assert_eq!(maker.library, Library::MakerWorld);
        assert_eq!(maker.id, "1400373");
        assert_eq!(maker.profile_id.as_deref(), Some("4242"));

        let thing = parse_model_url("https://www.thingiverse.com/thing:763622").unwrap();
        assert_eq!(thing.library, Library::Thingiverse);
        assert_eq!(thing.id, "763622");

        let bare = parse_model_url("thing:42").unwrap();
        assert_eq!(bare.id, "42");

        assert!(parse_model_url("https://example.com/models/1").is_err());
        assert!(parse_model_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn makerworld_search_names_the_creator_and_the_licence() {
        let raw: Value =
            serde_json::from_str(include_str!("fixtures/makerworld-search.json")).unwrap();
        let page = makerworld::parse_search(&raw).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.hits[0].creator, "Ada Maker");
        assert_eq!(
            page.hits[0].licence,
            "Creative Commons Attribution-NonCommercial 4.0"
        );
        assert_eq!(
            page.hits[0].page_url,
            "https://makerworld.com/en/models/1400373-cable-clip"
        );
        assert_eq!(page.hits[1].licence, "Not stated");
        assert_eq!(page.hits[1].creator, "Bo");
    }

    #[test]
    fn a_makerworld_listing_keeps_print_profiles() {
        let raw: Value =
            serde_json::from_str(include_str!("fixtures/makerworld-design.json")).unwrap();
        let listing = makerworld::parse_listing(&raw).unwrap();
        assert_eq!(listing.remote_model_id, "US300293b2858c3b");
        assert_eq!(listing.profiles.len(), 2);
        assert_eq!(listing.profiles[0].id, "4242");
        assert_eq!(listing.profiles[0].printer.as_deref(), Some("Bambu Lab A1"));
        assert!(listing.summary.contains("desk edge"));
    }

    #[test]
    fn thingiverse_search_needs_nothing_but_the_recorded_payload() {
        let raw: Value =
            serde_json::from_str(include_str!("fixtures/thingiverse-search.json")).unwrap();
        let page = thingiverse::parse_search(&raw).unwrap();
        assert_eq!(page.hits[0].creator, "Octo");
        assert_eq!(
            page.hits[0].licence,
            "Creative Commons Attribution-NonCommercial-ShareAlike 4.0"
        );
        assert_eq!(page.hits[0].id, "763622");
    }

    #[test]
    fn search_uses_the_fixture_and_caches_the_second_call() {
        let script = answers(&[include_str!("fixtures/makerworld-search.json")]);
        let client = browser(script.clone(), Credentials::default());
        let first = client
            .search("clip", Library::MakerWorld, 1, Sort::Relevant)
            .unwrap();
        let second = client
            .search("clip", Library::MakerWorld, 1, Sort::Relevant)
            .unwrap();
        assert_eq!(first.hits[0].title, "Cable clip");
        assert_eq!(second.hits.len(), first.hits.len());
        assert_eq!(script.seen().len(), 1);
        let url = &script.seen()[0].url;
        assert!(url.contains("select/design2"), "{url}");
        assert!(url.contains("keyword=clip"), "{url}");
        assert!(!client.cache.to_string_lossy().contains("catalog-cache"));
    }

    #[test]
    fn thingiverse_without_a_token_does_not_call_out() {
        let script = answers(&[]);
        let client = browser(script.clone(), Credentials::default());
        let err = client
            .search("octopus", Library::Thingiverse, 1, Sort::Relevant)
            .unwrap_err();
        assert!(matches!(err, ModelError::NeedsThingiverseToken), "{err}");
        assert!(script.seen().is_empty());
        assert!(err.to_string().contains("thingiverse.com/developers"));
    }

    #[test]
    fn a_token_is_a_header_and_never_part_of_the_url_or_the_debug_line() {
        let script = answers(&[include_str!("fixtures/thingiverse-search.json")]);
        let client = browser(
            script.clone(),
            Credentials {
                thingiverse_token: Some(TOKEN.into()),
                bambu_token: Some(TOKEN.into()),
            },
        );
        client
            .search("octopus", Library::Thingiverse, 1, Sort::Popular)
            .unwrap();
        let request = &script.seen()[0];
        assert!(!request.url.contains(TOKEN), "{}", request.url);
        let auth = request
            .headers
            .iter()
            .find(|(name, _)| name == "Authorization")
            .map(|(_, value)| value.as_str());
        assert_eq!(auth, Some(format!("Bearer {TOKEN}").as_str()));
        let debug = format!("{client:?}");
        assert!(!debug.contains(TOKEN), "{debug}");
        assert!(debug.contains("set"), "{debug}");
    }

    #[test]
    fn an_error_body_does_not_keep_the_token() {
        let script = Script::new(vec![Response::from_parts(
            500,
            format!("leaked {TOKEN}").into_bytes(),
        )]);
        let client = browser(
            script,
            Credentials {
                thingiverse_token: Some(TOKEN.into()),
                bambu_token: None,
            },
        );
        let err = client
            .search("octopus", Library::Thingiverse, 1, Sort::Relevant)
            .unwrap_err();
        let text = err.to_string();
        assert!(!text.contains(TOKEN), "{text}");
        assert!(text.contains("[redacted]"), "{text}");
    }

    #[test]
    fn makerworld_download_uses_the_account_token_then_fetches_the_file_without_it() {
        let profile = include_str!("fixtures/makerworld-profile.json");
        let design = include_str!("fixtures/makerworld-design.json");
        let file = Response::from_parts(200, {
            let mut bytes = b"PK\x03\x04".to_vec();
            bytes.extend(std::iter::repeat_n(0u8, 120));
            bytes
        });
        let script = Script::new(vec![file, json_answer(profile), json_answer(design)]);
        let client = browser(
            script.clone(),
            Credentials {
                bambu_token: Some(TOKEN.into()),
                thingiverse_token: None,
            },
        );
        let saved = client
            .download(Library::MakerWorld, "1400373", Some("4242"), None)
            .unwrap();
        assert!(saved.path.ends_with("Cable_clip.3mf"), "{}", saved.path);
        assert_eq!(saved.creator, "Ada Maker");
        assert!(saved.licence.contains("NonCommercial"), "{}", saved.licence);
        let note = fs::read_to_string(
            Path::new(&saved.path)
                .parent()
                .unwrap()
                .join("attribution.txt"),
        )
        .unwrap();
        assert!(note.contains("Ada Maker"), "{note}");
        assert!(note.contains("Licence:"), "{note}");
        let seen = script.seen();
        assert_eq!(seen.len(), 3);
        assert!(seen[0].url.contains("/design/1400373"), "{}", seen[0].url);
        assert!(seen[0]
            .headers
            .iter()
            .all(|(name, _)| name != "Authorization"));
        assert!(seen[1].url.contains("profile/4242"), "{}", seen[1].url);
        assert!(
            seen[1].url.contains("model_id=US300293b2858c3b"),
            "{}",
            seen[1].url
        );
        assert!(seen[1]
            .headers
            .iter()
            .any(|(name, value)| name == "Authorization" && value == &format!("Bearer {TOKEN}")));
        assert!(
            seen[2].url.starts_with("http://127.0.0.1/clip.3mf"),
            "{}",
            seen[2].url
        );
        assert!(seen[2]
            .headers
            .iter()
            .all(|(name, _)| name != "Authorization"));
        let _ = fs::remove_dir_all(&client.cache);
    }

    #[test]
    fn makerworld_download_without_an_account_stops_before_the_file() {
        let script = answers(&[include_str!("fixtures/makerworld-design.json")]);
        let client = browser(script.clone(), Credentials::default());
        let err = client
            .download(Library::MakerWorld, "1400373", None, None)
            .unwrap_err();
        assert!(matches!(err, ModelError::NeedsBambuSignIn), "{err}");
        assert!(script
            .seen()
            .iter()
            .all(|request| !request.url.contains("iot-service")));
    }

    #[test]
    fn thingiverse_import_follows_one_redirect_and_writes_the_stl() {
        let thing = include_str!("fixtures/thingiverse-thing.json");
        let files = include_str!("fixtures/thingiverse-files.json")
            .replace("http://127.0.0.1/octopus.stl", "http://127.0.0.1/start");
        let mut redirect = Response::from_parts(302, Vec::new());
        redirect.headers.push((
            "location".into(),
            "http://127.0.0.1/octopus-final.stl".into(),
        ));
        let stl = Response::from_parts(200, TINY_STL.as_bytes().to_vec());
        let script = Script::new(vec![stl, redirect, json_answer(&files), json_answer(thing)]);
        let client = browser(
            script.clone(),
            Credentials {
                thingiverse_token: Some(TOKEN.into()),
                bambu_token: None,
            },
        );
        let saved = client
            .import_url("https://www.thingiverse.com/thing:763622", None, None)
            .unwrap();
        assert!(saved.file_name.ends_with(".stl"), "{}", saved.file_name);
        assert_eq!(saved.creator, "Octo");
        assert!(saved.licence.contains("ShareAlike"), "{}", saved.licence);
        let seen = script.seen();
        assert_eq!(
            seen.len(),
            4,
            "{:?}",
            seen.iter().map(|item| item.url.clone()).collect::<Vec<_>>()
        );
        assert!(seen[2].url.contains("/start"), "{}", seen[2].url);
        assert!(seen[3].url.contains("octopus-final.stl"), "{}", seen[3].url);
        assert!(fs::read(&saved.path).unwrap().starts_with(b"solid clip"));
        let _ = fs::remove_dir_all(&client.cache);
    }

    #[test]
    fn a_blocked_makerworld_search_is_not_an_empty_page() {
        let script = answers(&[r#"{"total": 10, "hits": null}"#]);
        let client = browser(script, Credentials::default());
        let err = client
            .search("clip", Library::MakerWorld, 1, Sort::Relevant)
            .unwrap_err();
        assert!(matches!(err, ModelError::Blocked), "{err}");
    }

    #[test]
    fn the_standard_digital_file_licence_is_named_in_british_english() {
        assert_eq!(
            licence_label("Standard Digital File License"),
            "Standard Digital File Licence"
        );
    }
}
