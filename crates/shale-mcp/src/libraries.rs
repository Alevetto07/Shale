//! Tokens for the model libraries.
//!
//! Thingiverse uses the app token the user saved in Shale, or
//! `SHALE_THINGIVERSE_TOKEN`. MakerWorld downloads use the Bambu Cloud access
//! token the desktop saved. On Windows that is the Credential Manager entry
//! Shale already writes. This process does not ask for a password.

use std::sync::Arc;
use std::time::Duration;

use shale_host::cloud::HttpTransport;
#[cfg(windows)]
use shale_host::cloud::SecretStore;
use shale_host::models::{Browser, Credentials, Pace};

pub fn browser() -> Browser<HttpTransport> {
    Browser::new(
        Arc::new(HttpTransport),
        shale_host::models::cache_dir(),
        credentials(),
        Pace::shared(Duration::from_millis(400)),
    )
}

fn credentials() -> Credentials {
    Credentials {
        thingiverse_token: thingiverse_token(),
        bambu_token: bambu_token(),
    }
}

fn thingiverse_token() -> Option<String> {
    if let Ok(token) = std::env::var("SHALE_THINGIVERSE_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }
    #[cfg(windows)]
    if let Ok(Some(token)) = Vault.get("thingiverse.token") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }
    let path = shale_core::config_dir().join("thingiverse-token");
    shale_host::models::read_token_file(&path).ok().flatten()
}

fn bambu_token() -> Option<String> {
    #[cfg(windows)]
    {
        let mut cloud = shale_host::cloud::Cloud::new(
            shale_host::cloud::api::GLOBAL_HOST,
            Arc::new(HttpTransport),
            Arc::new(Vault),
        );
        cloud.load();
        if cloud.signed_in() {
            cloud.bearer().ok()
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
struct Vault;

#[cfg(windows)]
impl SecretStore for Vault {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new("Shale", key) {
            Ok(entry) => match entry.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(err) => Err(err.to_string()),
            },
            Err(err) => Err(err.to_string()),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new("Shale", key)
            .and_then(|entry| entry.set_password(value))
            .map_err(|err| err.to_string())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match keyring::Entry::new("Shale", key).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        }
    }
}
