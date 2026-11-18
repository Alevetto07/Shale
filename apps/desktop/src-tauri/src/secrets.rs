//! Passwords, kept where Windows keeps passwords.
//!
//! Two things live here: a Bambu machine's LAN access code, which is the
//! password on its MQTT connection and lets anyone who has it drive the
//! printer, and the cloud account's tokens. Both used to sit in the WebView's
//! `localStorage`, which is a plain file in the profile directory — readable by
//! anything running as this user, and backed up with the profile. They go to
//! the Windows Credential Manager now, through `keyring`: service `Shale`, and
//! the key as the account name, so a listing of the vault shows whose password
//! it is.
//!
//! Nothing here logs a value, and `get` returns `None` rather than an error when
//! the entry is absent — an absent password is a state the UI has to handle
//! anyway.
//!
//! The entries written by the previous build used the whole `Shale/<key>` string
//! as the target name, with no account. That is a different entry to Windows,
//! so an access code saved before this change has to be typed once more; the
//! alternative was keeping two ways into one vault.

#[cfg(windows)]
mod platform {
    use keyring::Entry;

    const SERVICE: &str = "Shale";

    fn entry(key: &str) -> Result<Entry, String> {
        Entry::new(SERVICE, key)
            .map_err(|err| format!("the credential manager refused the name: {err}"))
    }

    pub fn set(key: &str, value: &str) -> Result<(), String> {
        entry(key)?
            .set_password(value)
            .map_err(|err| format!("the credential manager refused the write: {err}"))
    }

    pub fn get(key: &str) -> Result<Option<String>, String> {
        match entry(key)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            // Absent is an answer, not a failure.
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(format!("the credential manager refused the read: {err}")),
        }
    }

    pub fn clear(key: &str) -> Result<(), String> {
        match entry(key)?.delete_credential() {
            // Clearing twice is not an error: the state asked for is the state
            // that is there.
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("the credential manager refused the delete: {err}")),
        }
    }
}

#[cfg(not(windows))]
mod platform {
    // `keyring` needs a platform backend chosen at build time. Until this
    // program is built somewhere that is not Windows, saying so is better than
    // a build that quietly has no vault.
    pub fn set(_key: &str, _value: &str) -> Result<(), String> {
        Err("there is no keyring on this platform yet".to_string())
    }
    pub fn get(_key: &str) -> Result<Option<String>, String> {
        Err("there is no keyring on this platform yet".to_string())
    }
    pub fn clear(_key: &str) -> Result<(), String> {
        Err("there is no keyring on this platform yet".to_string())
    }
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    platform::set(key, value)
}

pub fn get(key: &str) -> Result<Option<String>, String> {
    platform::get(key)
}

pub fn clear(key: &str) -> Result<(), String> {
    platform::clear(key)
}

#[cfg(all(test, windows))]
mod tests {
    use super::{clear, get, set};

    /// One key per run, so a leftover entry from a failed run cannot make the
    /// next one pass.
    fn key() -> String {
        format!("test/{}", std::process::id())
    }

    #[test]
    fn a_password_survives_a_round_trip_and_a_clear() {
        let key = key();
        let secret = "not-a-real-access-code";
        set(&key, secret).expect("the vault took the write");
        assert_eq!(
            get(&key).expect("the vault answered"),
            Some(secret.to_string())
        );
        clear(&key).expect("the vault took the delete");
        assert_eq!(
            get(&key).expect("the vault answered after the delete"),
            None,
            "a cleared entry must read as absent, not as an error"
        );
        // Clearing twice is not an error: the state asked for is the state there.
        clear(&key).expect("clearing an absent entry is fine");
    }
}
