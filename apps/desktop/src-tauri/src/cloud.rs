//! The window's side of Bambu's cloud: one account, one device list, one send.
//!
//! Everything that knows a URL lives in `shale_host::cloud::api`; this file
//! only holds the account for the life of the process, puts the tokens in the
//! Windows Credential Manager through the same helper the printer's access code
//! uses, and turns the module's errors into strings the pane can show.
//!
//! `SHALE_CLOUD_HOST` points the client somewhere else — a loopback server, in
//! the probe — and exists for the same reason `SHALE_CURA_DIR` does: a thing
//! that can only be tested against the real service is a thing that is not
//! tested.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use shale_host::cloud::{api, devices, Cloud, HttpTransport, SecretStore};

/// The vault, as `SecretStore`. Keys are already scoped by `Shale/` inside the
/// secrets module, so a token and the printer's access code never collide.
pub struct Vault;

impl SecretStore for Vault {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        crate::secrets::get(key)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        crate::secrets::set(key, value)
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        crate::secrets::clear(key)
    }
}

pub struct CloudState(pub Mutex<Cloud<HttpTransport>>);

pub fn state() -> CloudState {
    let host = std::env::var("SHALE_CLOUD_HOST").unwrap_or_else(|_| api::GLOBAL_HOST.to_string());
    // A probe signs in against a loopback server with a made-up account; it must
    // not leave those tokens in the real vault, where the next real sign-in
    // would find them and try to refresh somebody's fixture.
    let store: Arc<dyn SecretStore> =
        if std::env::var("SHALE_CLOUD_STORE").as_deref() == Ok("memory") {
            Arc::new(shale_host::cloud::MemoryStore::default())
        } else {
            Arc::new(Vault)
        };
    let mut cloud = Cloud::new(&host, Arc::new(HttpTransport), store);
    cloud.load();
    CloudState(Mutex::new(cloud))
}

fn lock(state: &CloudState) -> Result<std::sync::MutexGuard<'_, Cloud<HttpTransport>>, String> {
    state
        .0
        .lock()
        .map_err(|_| "the cloud session is in an unknown state; restart Shale".to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub signed_in: bool,
    pub account: Option<String>,
    pub host: String,
}

#[tauri::command]
pub fn cloud_state(state: tauri::State<CloudState>) -> Result<Session, String> {
    let cloud = lock(&state)?;
    Ok(Session {
        signed_in: cloud.signed_in(),
        account: cloud.account().map(str::to_string),
        host: cloud.host().to_string(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStep {
    /// True when Bambu wants the six-digit code before it will hand out tokens.
    pub needs_code: bool,
    pub tfa_key: Option<String>,
}

/// Sign in with the address and the password. When Bambu asks for a code, the
/// code is sent to that address before this returns, so the pane only has to
/// show the field.
#[tauri::command]
pub fn cloud_login(
    state: tauri::State<CloudState>,
    email: String,
    password: String,
) -> Result<LoginStep, String> {
    let mut cloud = lock(&state)?;
    match cloud
        .sign_in(&email, &password)
        .map_err(|err| err.to_string())?
    {
        shale_host::cloud::SignIn::SignedIn => Ok(LoginStep {
            needs_code: false,
            tfa_key: None,
        }),
        shale_host::cloud::SignIn::NeedsCode { tfa_key } => {
            // A failure to send the code is worth saying: the field would
            // otherwise sit there waiting for a code that is not coming.
            cloud.send_code(&email).map_err(|err| err.to_string())?;
            Ok(LoginStep {
                needs_code: true,
                tfa_key: Some(tfa_key),
            })
        }
    }
}

#[tauri::command]
pub fn cloud_verify(
    state: tauri::State<CloudState>,
    email: String,
    tfa_key: String,
    code: String,
) -> Result<(), String> {
    let mut cloud = lock(&state)?;
    cloud
        .submit_code(&email, &tfa_key, &code)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn cloud_sign_out(state: tauri::State<CloudState>) -> Result<(), String> {
    let mut cloud = lock(&state)?;
    cloud.sign_out().map_err(|err| err.to_string())
}

/// One row per printer, already sorted so the machines that can take a print
/// come first. The state is a phrase, not an enum, because the pane only shows
/// it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRow {
    pub dev_id: String,
    pub label: String,
    pub model: String,
    pub state: String,
    pub may_print: bool,
}

#[tauri::command]
pub fn cloud_devices(state: tauri::State<CloudState>) -> Result<Vec<DeviceRow>, String> {
    let mut cloud = lock(&state)?;
    let list = cloud.devices().map_err(|err| err.to_string())?;
    Ok(devices::rows(&list)
        .into_iter()
        .map(|row| DeviceRow {
            dev_id: row.dev_id,
            label: row.label,
            model: row.model,
            state: row.state.line(),
            may_print: row.state.may_print(),
        })
        .collect())
}

/// Send the plate that was sliced last. The G-code is read from the file the
/// slicer wrote, not from the window: a 500 KB string through the IPC boundary
/// is a copy nobody needs.
#[tauri::command]
pub fn cloud_send(
    state: tauri::State<CloudState>,
    app: tauri::State<crate::AppState>,
    dev_id: String,
    task_name: String,
) -> Result<String, String> {
    let path = crate::last_gcode_path(&app)?
        .ok_or_else(|| "there is nothing to send: slice the plate first".to_string())?;
    let gcode = std::fs::read(&path)
        .map_err(|err| format!("cannot read the G-code at {}: {err}", path.display()))?;
    let mut cloud = lock(&state)?;
    cloud
        .send_print(&dev_id, &task_name, &gcode)
        .map_err(|err| err.to_string())
}
