//! The printer's access code, kept where Windows keeps passwords.
//!
//! A Bambu machine's LAN access code is the password on its MQTT connection:
//! anyone with it can drive the printer. It used to sit in the WebView's
//! `localStorage`, which is a plain file in the profile directory — readable by
//! anything running as this user, and backed up with the profile. It goes to the
//! Windows Credential Manager now, one entry per key, under the target name
//! `Shale/<key>`.
//!
//! Nothing here logs a value, and `get` returns `None` rather than an error when
//! the entry is absent — an absent password is a state the UI has to handle
//! anyway.

/// The target name a key is stored under. Scoped by `Shale/` so a listing of the
/// vault shows whose password it is.
fn target(key: &str) -> String {
    format!("Shale/{key}")
}

#[cfg(windows)]
mod platform {
    use super::target;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    /// A NUL-terminated UTF-16 buffer, which is what every `*W` call wants.
    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn error(what: &str) -> String {
        // SAFETY: `GetLastError` takes no arguments and reads thread-local state.
        let code = unsafe { GetLastError() };
        format!("{what} failed (Windows error {code})")
    }

    pub fn set(key: &str, value: &str) -> Result<(), String> {
        let name = wide(&target(key));
        let mut blob = value.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_ptr() as *mut u16,
            Comment: std::ptr::null_mut(),
            LastWritten: windows_sys::Win32::Foundation::FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: std::ptr::null_mut(),
        };
        // SAFETY: the struct and both buffers outlive the call, the blob length is
        // the real length, and nothing is handed back to be freed.
        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            return Err(error("saving the printer's access code"));
        }
        Ok(())
    }

    pub fn get(key: &str) -> Result<Option<String>, String> {
        let name = wide(&target(key));
        let mut out: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `out` is a valid pointer to a pointer, and the credential it
        // receives is owned by the vault and freed with `CredFree` below.
        let ok = unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut out) };
        if ok == 0 {
            // SAFETY: as above; the read failed, so there is nothing to free.
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(error("reading the printer's access code"));
        }
        // SAFETY: a successful read fills `out` with a live credential.
        let credential = unsafe { &*out };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        };
        let text = String::from_utf8_lossy(bytes).to_string();
        // SAFETY: the buffer is the vault's and is released exactly once.
        unsafe { CredFree(out as *const core::ffi::c_void) };
        Ok(Some(text))
    }

    pub fn clear(key: &str) -> Result<(), String> {
        let name = wide(&target(key));
        // SAFETY: `name` is NUL-terminated and lives for the call.
        let ok = unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            // SAFETY: as above.
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(());
            }
            return Err(error("forgetting the printer's access code"));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod platform {
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
