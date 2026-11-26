//! Bambu's cloud: an account, a device list, and sending a print.
//!
//! Three rules hold this module together.
//!
//! * The undocumented part lives in [`api`] and nowhere else. Everything else
//!   in Shale calls [`Cloud`], and every error names the call that failed.
//! * Tokens never leave the store in a printable form. [`Tokens`] has a hand
//!   written `Debug` that says `access: set`, and no error message carries a
//!   token — there is a test for each.
//! * Nothing here decides when to talk to the cloud. The window asks; the CLI
//!   and the MCP server never do.

pub mod api;
pub mod devices;
pub mod http;
pub mod session;

use std::sync::Arc;

use serde_json::Value;

pub use api::{CloudDevice, LoginAnswer};
pub use session::{SignIn, Tokens};

/// A request on its way out, named so an error can say which call it was.
///
/// [`Debug`] redacts `Authorization`. The access token is never printed.
#[derive(Clone)]
pub struct HttpRequest {
    pub name: &'static str,
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(name, value)| {
                if name.eq_ignore_ascii_case("authorization") {
                    (name.as_str(), "Bearer [redacted]")
                } else {
                    (name.as_str(), value.as_str())
                }
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("name", &self.name)
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body_bytes", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

/// The seam that lets the tests answer without a network.
pub trait Transport: Send + Sync {
    fn send(&self, request: HttpRequest) -> Result<http::Response, String>;
}

/// The real one: HTTPS over rustls.
pub struct HttpTransport;

impl Transport for HttpTransport {
    fn send(&self, request: HttpRequest) -> Result<http::Response, String> {
        let headers: Vec<(&str, String)> = request
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone()))
            .collect();
        http::send(request.method, &request.url, &headers, request.body)
            .map_err(|err| err.to_string())
    }
}

/// Where the tokens live. On Windows this is the Credential Manager, reached
/// through the same helper the printer's access code uses; in tests it is a
/// map. The trait is here so the cloud module can be tested without one.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// The store used by tests, and by a build with no credential vault.
#[derive(Default)]
pub struct MemoryStore {
    values: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Everything that can go wrong, said in terms of the call and the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudError {
    /// The connection failed before there was an answer.
    Unreachable { call: String, message: String },
    /// Bambu answered with a status that is not 2xx.
    Status {
        call: String,
        status: u16,
        message: String,
    },
    /// Bambu answered 2xx with something that is not what was expected.
    Shape { call: String, message: String },
    /// No tokens, or the refresh token is gone too.
    SignedOut,
    /// The store refused to keep or return a token.
    Store(String),
    /// The caller asked for something that is not possible yet.
    Refused(String),
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable { call, message } => {
                write!(f, "{call} could not be reached: {message}")
            }
            Self::Status {
                call,
                status,
                message,
            } => {
                if *status == 404 {
                    write!(
                        f,
                        "Bambu's cloud did not recognise that request. Sign out and sign in again if this account uses the China cloud."
                    )
                } else if message.is_empty() || message.starts_with('{') {
                    write!(f, "Bambu's cloud refused {call} ({status}).")
                } else {
                    write!(f, "{call} was refused ({status}): {message}")
                }
            }
            Self::Shape { call, message } => {
                write!(f, "{call} answered something unexpected: {message}")
            }
            Self::SignedOut => f.write_str("not signed in to Bambu's cloud"),
            Self::Store(message) => write!(f, "the credential store refused: {message}"),
            Self::Refused(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CloudError {}

impl From<CloudError> for String {
    fn from(err: CloudError) -> Self {
        err.to_string()
    }
}

/// Seconds since the epoch, the only clock this module trusts.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// The account, as far as this process is concerned.
pub struct Cloud<T: Transport> {
    host: String,
    transport: Arc<T>,
    store: Arc<dyn SecretStore>,
    tokens: Option<Tokens>,
    /// The account name, kept so the window can say who is signed in without
    /// asking the cloud. Not a secret.
    account: Option<String>,
}

const KEY_ACCESS: &str = "cloud.access";
const KEY_REFRESH: &str = "cloud.refresh";
const KEY_EXPIRES: &str = "cloud.expires";
const KEY_ACCOUNT: &str = "cloud.account";
const KEY_HOST: &str = "cloud.host";

impl<T: Transport> Cloud<T> {
    pub fn new(host: &str, transport: Arc<T>, store: Arc<dyn SecretStore>) -> Self {
        Self {
            host: host.to_string(),
            transport,
            store,
            tokens: None,
            account: None,
        }
    }

    /// Read whatever the last run left behind. A store that fails here is not
    /// fatal: the window shows signed out and signing in will try again.
    pub fn load(&mut self) {
        self.apply_stored_host();
        let access = self.store.get(KEY_ACCESS).ok().flatten();
        let refresh = self.store.get(KEY_REFRESH).ok().flatten();
        let expires = self
            .store
            .get(KEY_EXPIRES)
            .ok()
            .flatten()
            .and_then(|text| text.parse::<i64>().ok());
        self.account = self.store.get(KEY_ACCOUNT).ok().flatten();
        self.tokens = match (access, refresh, expires) {
            (Some(access), Some(refresh), Some(expires)) => {
                Some(Tokens::new(&access, &refresh, expires))
            }
            _ => None,
        };
    }

    pub fn account(&self) -> Option<&str> {
        self.account.as_deref()
    }

    pub fn signed_in(&self) -> bool {
        self.tokens.is_some()
    }

    /// Store tokens from Bambu's web sign-in the same way a password sign-in does.
    pub fn adopt_web_login(
        &mut self,
        access: &str,
        refresh: &str,
        expires_in: i64,
        account: &str,
    ) -> Result<(), CloudError> {
        let expires_at = now_unix() + expires_in.max(60);
        self.remember(Tokens::new(access, refresh, expires_at), account)
    }

    /// Exchange the ticket Bambu's login page sends to the localhost callback.
    /// A failed answer is reported by status only, so the ticket and the tokens
    /// never appear in the error.
    pub fn complete_web_ticket(&mut self, ticket: &str) -> Result<(), CloudError> {
        let response = self.exchange_ticket(ticket)?;
        let (access, refresh, expires_in) =
            api::tokens_from_ticket_body(&response.body).ok_or_else(|| CloudError::Shape {
                call: "POST /v1/user-service/user/ticket".into(),
                message: "the answer carried no tokens".into(),
            })?;
        let account = match self
            .transport
            .send(api::my_profile_request(&self.host, &access))
        {
            Ok(profile) if (200..300).contains(&profile.status) => {
                api::account_from_profile(&profile.body).unwrap_or_else(|| "Bambu account".into())
            }
            _ => "Bambu account".into(),
        };
        self.adopt_web_login(&access, &refresh, expires_in, &account)
    }

    fn exchange_ticket(&self, ticket: &str) -> Result<http::Response, CloudError> {
        let call = "POST /v1/user-service/user/ticket";
        let first = self
            .transport
            .send(api::ticket_exchange_request(&self.host, ticket))
            .map_err(|message| CloudError::Unreachable {
                call: call.into(),
                message,
            })?;
        if (200..300).contains(&first.status) {
            return Ok(first);
        }
        match self
            .transport
            .send(api::ticket_exchange_body_request(&self.host, ticket))
        {
            Ok(second) if (200..300).contains(&second.status) => Ok(second),
            Ok(second) => Err(CloudError::Status {
                call: call.into(),
                status: second.status,
                message: "Bambu did not accept the sign-in".into(),
            }),
            Err(message) => Err(CloudError::Unreachable {
                call: call.into(),
                message,
            }),
        }
    }

    fn remember(&mut self, tokens: Tokens, account: &str) -> Result<(), CloudError> {
        self.store
            .set(KEY_ACCESS, &tokens.access)
            .map_err(CloudError::Store)?;
        self.store
            .set(KEY_REFRESH, &tokens.refresh)
            .map_err(CloudError::Store)?;
        self.store
            .set(KEY_EXPIRES, &tokens.expires_at.to_string())
            .map_err(CloudError::Store)?;
        self.store
            .set(KEY_ACCOUNT, account)
            .map_err(CloudError::Store)?;
        self.account = Some(account.to_string());
        self.tokens = Some(tokens);
        self.store_host()?;
        Ok(())
    }

    pub fn sign_out(&mut self) -> Result<(), CloudError> {
        for key in [KEY_ACCESS, KEY_REFRESH, KEY_EXPIRES, KEY_ACCOUNT] {
            self.store.delete(key).map_err(CloudError::Store)?;
        }
        self.tokens = None;
        self.account = None;
        Ok(())
    }

    /// The access token, for a call the window or a tool asked for. Refreshed
    /// when it is about to expire. Callers must not log the string.
    pub fn bearer(&mut self) -> Result<String, CloudError> {
        self.access_token()
    }

    /// The access token, refreshed if it is about to expire. Five minutes of
    /// slack: a slice takes longer than the round trip, and a token that dies
    /// mid-upload is a print that does not start.
    fn access_token(&mut self) -> Result<String, CloudError> {
        let Some(tokens) = self.tokens.clone() else {
            return Err(CloudError::SignedOut);
        };
        if tokens.expires_at - 300 > now_unix() {
            return Ok(tokens.access);
        }
        let request = api::refresh(&tokens.refresh).into_request(&self.host, None);
        let answer = self.send(request)?;
        let parsed: LoginAnswer = parse(&answer, "POST /v1/user-service/user/refreshtoken")?;
        let (access, refresh) = parsed.tokens().ok_or_else(|| CloudError::Shape {
            call: "POST /v1/user-service/user/refreshtoken".into(),
            message: "the answer carried no tokens; signing in again is the only way forward"
                .into(),
        })?;
        let refreshed = Tokens::new(
            access,
            refresh,
            now_unix() + parsed.expires_in.unwrap_or(86_400),
        );
        let account = self.account.clone().unwrap_or_default();
        self.remember(refreshed, &account)?;
        Ok(self
            .tokens
            .as_ref()
            .map(|t| t.access.clone())
            .unwrap_or_default())
    }

    fn send(&self, request: HttpRequest) -> Result<http::Response, CloudError> {
        let response =
            self.transport
                .send(request.clone())
                .map_err(|message| CloudError::Unreachable {
                    call: request.name.to_string(),
                    message,
                })?;
        if !(200..300).contains(&response.status) {
            return Err(CloudError::Status {
                call: request.name.to_string(),
                status: response.status,
                message: redact(&message_of(&response.body)),
            });
        }
        Ok(response)
    }

    // ------------------------------------------------------------ sign in

    /// Email and password. Bambu may answer with tokens or with a request for a
    /// code; both are ordinary outcomes, not errors.
    pub fn sign_in(&mut self, email: &str, password: &str) -> Result<SignIn, CloudError> {
        let request = api::login(email, password).into_request(&self.host, None);
        let answer = self.send(request)?;
        let parsed: LoginAnswer = parse(&answer, "POST /v1/user-service/user/login")?;
        self.outcome(parsed, email)
    }

    /// Ask for a code to be sent to the address. Bambu answers with an empty
    /// object; a failure here is usually a wrong address or a rate limit.
    pub fn send_code(&self, email: &str) -> Result<(), CloudError> {
        let request = api::send_code(email).into_request(&self.host, None);
        self.send(request).map(|_| ())
    }

    /// The code the person typed, with the key from the first answer.
    pub fn submit_code(
        &mut self,
        email: &str,
        tfa_key: &str,
        code: &str,
    ) -> Result<SignIn, CloudError> {
        let request = api::login_with_code(email, code, tfa_key).into_request(&self.host, None);
        let answer = self.send(request)?;
        let parsed: LoginAnswer = parse(&answer, "POST /v1/user-service/user/login (code)")?;
        self.outcome(parsed, email)
    }

    fn outcome(&mut self, parsed: LoginAnswer, email: &str) -> Result<SignIn, CloudError> {
        if parsed.wants_code() {
            return Ok(SignIn::NeedsCode {
                tfa_key: parsed.tfa_key.unwrap_or_default(),
            });
        }
        let (access, refresh) = parsed.tokens().ok_or_else(|| CloudError::Shape {
            call: "POST /v1/user-service/user/login".into(),
            message: "the answer carried neither tokens nor a code request".into(),
        })?;
        let tokens = Tokens::new(
            access,
            refresh,
            now_unix() + parsed.expires_in.unwrap_or(86_400),
        );
        self.remember(tokens, email)?;
        Ok(SignIn::SignedIn)
    }

    // ------------------------------------------------------------ devices

    pub fn devices(&mut self) -> Result<Vec<CloudDevice>, CloudError> {
        let token = self.access_token()?;
        let request = api::devices().into_request(&self.host, Some(&token));
        let answer = self.send(request)?;
        let list: api::DeviceList = parse(&answer, "GET /v1/iot-service/api/user/bind")?;
        Ok(list.devices)
    }

    /// Recent tasks from `GET /v1/user-service/my/tasks`. Read only.
    pub fn tasks(&mut self) -> Result<Vec<api::CloudTask>, CloudError> {
        let token = self.access_token()?;
        let request = api::tasks().into_request(&self.host, Some(&token));
        let answer = self.send(request)?;
        let list: api::TaskList = parse(&answer, "GET /v1/user-service/my/tasks")?;
        Ok(list.hits)
    }

    /// Send a sliced file to one printer through the cloud, and return the task
    /// id the cloud gave it.
    pub fn send_print(
        &mut self,
        dev_id: &str,
        task_name: &str,
        gcode: &[u8],
    ) -> Result<String, CloudError> {
        if dev_id.trim().is_empty() {
            return Err(CloudError::Refused(
                "no printer was chosen for this print".into(),
            ));
        }
        if gcode.is_empty() {
            return Err(CloudError::Refused("there is nothing to print".into()));
        }
        self.send_print_named(dev_id, task_name, "plate.gcode", gcode)
    }

    /// Send a sliced file under an explicit name. A Bambu machine takes
    /// `plate.gcode.3mf`; `send_print` keeps the plain G-code name.
    pub fn send_print_named(
        &mut self,
        dev_id: &str,
        task_name: &str,
        file_name: &str,
        file: &[u8],
    ) -> Result<String, CloudError> {
        if dev_id.trim().is_empty() {
            return Err(CloudError::Refused(
                "no printer was chosen for this print".into(),
            ));
        }
        if file.is_empty() {
            return Err(CloudError::Refused("there is nothing to print".into()));
        }
        self.send_print_task(dev_id, task_name, file_name, file, &serde_json::json!({}))
    }

    /// Send a sliced file and attach the print fields (tray map, calibration)
    /// to the cloud task. The file itself still carries the same tray map.
    pub fn send_print_task(
        &mut self,
        dev_id: &str,
        task_name: &str,
        file_name: &str,
        file: &[u8],
        extra: &serde_json::Value,
    ) -> Result<String, CloudError> {
        if dev_id.trim().is_empty() {
            return Err(CloudError::Refused(
                "no printer was chosen for this print".into(),
            ));
        }
        if file.is_empty() {
            return Err(CloudError::Refused("there is nothing to print".into()));
        }
        let token = self.access_token()?;
        let request = api::create_task_with(dev_id, task_name, file_name, file, extra)
            .into_request(&self.host, Some(&token));
        let answer = self.send(request)?;
        let created: api::TaskCreated = parse(&answer, "POST /v1/iot-service/api/user/task")?;
        created.id().ok_or_else(|| CloudError::Shape {
            call: "POST /v1/iot-service/api/user/task".into(),
            message: "the task was accepted without an id".into(),
        })
    }

    /// The host this cloud is pointed at, for the window's diagnostics.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Point the next sign-in at the global cloud or the China cloud.
    ///
    /// A China account (`CN`, `China`, `zh-CN`) uses `api.bambulab.cn`. The
    /// two clouds do not share tokens, so changing region while signed in
    /// signs out. A loopback host set for a test is left alone and is not
    /// written into the store.
    pub fn set_region(&mut self, region: &str) -> Result<(), CloudError> {
        let next = api::host_for_region(region).to_string();
        if next != self.host && self.signed_in() {
            self.sign_out()?;
        }
        self.host = next;
        self.store_host()
    }

    fn apply_stored_host(&mut self) {
        let Ok(Some(stored)) = self.store.get(KEY_HOST) else {
            return;
        };
        if stored != api::GLOBAL_HOST && stored != api::CHINA_HOST {
            return;
        }
        if self.host == api::GLOBAL_HOST || self.host == api::CHINA_HOST {
            self.host = stored;
        }
    }

    fn store_host(&self) -> Result<(), CloudError> {
        if self.host == api::GLOBAL_HOST || self.host == api::CHINA_HOST {
            self.store
                .set(KEY_HOST, &self.host)
                .map_err(CloudError::Store)?;
        }
        Ok(())
    }
}

fn parse<R: serde::de::DeserializeOwned>(
    response: &http::Response,
    call: &str,
) -> Result<R, CloudError> {
    serde_json::from_slice(&response.body).map_err(|err| CloudError::Shape {
        call: call.to_string(),
        message: format!(
            "{err}; first bytes: {}",
            redact(&message_of(&response.body))
        ),
    })
}

/// Bambu's own message, or a readable slice of the body when it is not JSON.
fn message_of(body: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        for key in ["error", "message", "msg", "errorMessage", "error_msg"] {
            if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    return text.to_string();
                }
            }
        }
        return value.to_string();
    }
    let text = String::from_utf8_lossy(body);
    text.chars().take(200).collect()
}

/// Drop a body that carries a token or a printer access code. The window and
/// the logs must not repeat those.
fn redact(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("accesstoken")
        || lower.contains("access_token")
        || lower.contains("refreshtoken")
        || lower.contains("refresh_token")
        || lower.contains("dev_access_code")
        || lower.contains("authorization")
        || lower.contains("bearer ")
    {
        return "the cloud's answer was withheld because it contained a secret".into();
    }
    text.chars().take(180).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_that_contains_an_access_code_is_not_repeated() {
        let text = redact(r#"{"dev_access_code":"ACCESS-CODE-SECRET","accessToken":"access-secret-token"}"#);
        assert!(!text.contains("ACCESS-CODE-SECRET"), "{text}");
        assert!(!text.contains("access-secret-token"), "{text}");
    }

    #[test]
    fn tokens_never_print_themselves() {
        let tokens = Tokens::new("access-secret", "refresh-secret", 1);
        let text = format!("{tokens:?}");
        assert!(!text.contains("access-secret"), "{text}");
        assert!(!text.contains("refresh-secret"), "{text}");
    }

    #[test]
    fn a_store_failure_names_the_store_and_not_the_token() {
        struct Refusing;
        impl SecretStore for Refusing {
            fn get(&self, _: &str) -> Result<Option<String>, String> {
                Ok(None)
            }
            fn set(&self, _: &str, _: &str) -> Result<(), String> {
                Err("the vault is locked".into())
            }
            fn delete(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
        }
        let mut cloud = Cloud::new(
            api::GLOBAL_HOST,
            Arc::new(HttpTransport),
            Arc::new(Refusing),
        );
        let err = cloud
            .remember(Tokens::new("a", "r", 1), "a@b.c")
            .unwrap_err();
        assert!(err.to_string().contains("vault is locked"));
        assert!(!err.to_string().contains("a@b.c"));
    }

    #[test]
    fn web_login_is_stored_like_a_password_login() {
        let mut cloud = Cloud::new(
            api::GLOBAL_HOST,
            Arc::new(HttpTransport),
            Arc::new(MemoryStore::default()),
        );
        cloud
            .adopt_web_login("access-secret", "refresh-secret", 3600, "a@b.c")
            .unwrap();
        assert!(cloud.signed_in());
        assert_eq!(cloud.account(), Some("a@b.c"));
        let text = format!("{:?}", cloud.tokens);
        assert!(!text.contains("access-secret"), "{text}");
        assert!(!text.contains("refresh-secret"), "{text}");
    }
}
