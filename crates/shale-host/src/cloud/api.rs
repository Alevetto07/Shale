//! Everything Shale knows about Bambu's cloud, in one file.
//!
//! These endpoints are not documented and not versioned. They are the calls
//! Bambu Studio's own client makes, transcribed here so that the rest of Shale
//! never contains a URL, a field name or a magic string from that service. If
//! Bambu changes one, this file is the only place that has to change, and the
//! error the user sees names the call that failed rather than "login failed".
//!
//! Nothing here decides anything: it builds a request and reads an answer.

use serde::Deserialize;

use super::HttpRequest;

/// The two clouds. A European or American account lives on `.com`; an account
/// registered in mainland China lives on `.cn`, and the two do not share
/// tokens.
pub const GLOBAL_HOST: &str = "https://api.bambulab.com";
pub const CHINA_HOST: &str = "https://api.bambulab.cn";

/// A call, before it becomes bytes.
#[derive(Debug, Clone)]
pub struct Call {
    /// Where it goes, for the error message: `POST /v1/user-service/user/login`.
    pub name: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub body: Body,
}

#[derive(Debug, Clone)]
pub enum Body {
    None,
    Json(serde_json::Value),
    /// `(boundary, bytes)`: the multipart body is assembled here, complete.
    Multipart {
        boundary: String,
        bytes: Vec<u8>,
    },
}

impl Call {
    pub fn into_request(self, host: &str, token: Option<&str>) -> HttpRequest {
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if let Some(token) = token {
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
        let body = match self.body {
            Body::None => None,
            Body::Json(value) => {
                headers.push(("Content-Type".to_string(), "application/json".to_string()));
                Some(value.to_string().into_bytes())
            }
            Body::Multipart { boundary, bytes } => {
                headers.push((
                    "Content-Type".to_string(),
                    format!("multipart/form-data; boundary={boundary}"),
                ));
                Some(bytes)
            }
        };
        HttpRequest {
            name: self.name,
            method: self.method,
            url: format!("{host}{}", self.path),
            headers,
            body,
        }
    }
}

fn json(
    name: &'static str,
    method: &'static str,
    path: &'static str,
    value: serde_json::Value,
) -> Call {
    Call {
        name,
        method,
        path,
        body: Body::Json(value),
    }
}

// ---------------------------------------------------------------- sign in

/// Email and password. The answer is either tokens or a request for a code:
/// Bambu sends a six-digit code to the address when the account has two-factor
/// on, when the address is new to this machine, or when it decides to.
pub fn login(account: &str, password: &str) -> Call {
    json(
        "POST /v1/user-service/user/login",
        "POST",
        "/v1/user-service/user/login",
        serde_json::json!({ "account": account, "password": password, "apiError": "" }),
    )
}

/// Ask for the code to be sent. `codeLogin` is what the client sends for a
/// sign-in code, as opposed to `codeResetPassword`.
pub fn send_code(email: &str) -> Call {
    json(
        "POST /v1/user-service/user/sendemail/code",
        "POST",
        "/v1/user-service/user/sendemail/code",
        serde_json::json!({ "email": email, "type": "codeLogin" }),
    )
}

/// The code, with the `tfaKey` the first answer carried. The key is what ties
/// the code to the attempt that asked for it.
/// Bambu Studio exchanges the ticket from its localhost callback here. The
/// ticket is in the URL, never in the call name, so an error cannot echo it.
pub fn ticket_exchange_request(host: &str, ticket: &str) -> HttpRequest {
    HttpRequest {
        name: "POST /v1/user-service/user/ticket",
        method: "POST",
        url: format!(
            "{host}/v1/user-service/user/ticket/{}",
            path_segment(ticket)
        ),
        headers: vec![
            ("Accept".to_string(), "application/json".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ],
        body: Some(b"{}".to_vec()),
    }
}

/// The same exchange with the ticket in the body, used when the path form is
/// refused. The call name still does not contain the ticket.
pub fn ticket_exchange_body_request(host: &str, ticket: &str) -> HttpRequest {
    HttpRequest {
        name: "POST /v1/user-service/user/ticket",
        method: "POST",
        url: format!("{host}/v1/user-service/user/ticket"),
        headers: vec![
            ("Accept".to_string(), "application/json".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ],
        body: Some(serde_json::json!({ "ticket": ticket }).to_string().into_bytes()),
    }
}

pub fn my_profile_request(host: &str, access: &str) -> HttpRequest {
    HttpRequest {
        name: "GET /v1/user-service/my/profile",
        method: "GET",
        url: format!("{host}/v1/user-service/my/profile"),
        headers: vec![
            ("Accept".to_string(), "application/json".to_string()),
            ("Authorization".to_string(), format!("Bearer {access}")),
        ],
        body: None,
    }
}

fn path_segment(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Tokens from a ticket answer. Looks at the object and at `data`, and never
/// logs the body.
pub fn tokens_from_ticket_body(body: &[u8]) -> Option<(String, String, i64)> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    tokens_from_value(&value)
}

pub fn tokens_from_value(value: &serde_json::Value) -> Option<(String, String, i64)> {
    let object = value.as_object()?;
    let nested = object.get("data").and_then(|item| item.as_object());
    let source = nested.unwrap_or(object);
    let access = source
        .get("accessToken")
        .or_else(|| source.get("token"))
        .and_then(|item| item.as_str())
        .filter(|text| !text.is_empty())?;
    let refresh = source
        .get("refreshToken")
        .or_else(|| source.get("refresh_token"))
        .and_then(|item| item.as_str())
        .filter(|text| !text.is_empty())
        .unwrap_or(access);
    let expires = source
        .get("expiresIn")
        .and_then(|item| {
            item.as_i64()
                .or_else(|| item.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(86_400);
    Some((access.to_string(), refresh.to_string(), expires))
}

pub fn account_from_profile(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let object = value.as_object()?;
    let nested = object.get("data").and_then(|item| item.as_object());
    let source = nested.unwrap_or(object);
    for key in ["account", "email", "name"] {
        if let Some(text) = source.get(key).and_then(|item| item.as_str()) {
            if !text.trim().is_empty() {
                return Some(text.trim().to_string());
            }
        }
    }
    None
}

pub fn login_with_code(account: &str, code: &str, tfa_key: &str) -> Call {
    json(
        "POST /v1/user-service/user/login (code)",
        "POST",
        "/v1/user-service/user/login",
        serde_json::json!({ "account": account, "code": code, "tfaKey": tfa_key }),
    )
}

pub fn refresh(refresh_token: &str) -> Call {
    json(
        "POST /v1/user-service/user/refreshtoken",
        "POST",
        "/v1/user-service/user/refreshtoken",
        serde_json::json!({ "refreshToken": refresh_token }),
    )
}

pub fn profile() -> Call {
    Call {
        name: "GET /v1/iot-service/api/user/profile",
        method: "GET",
        path: "/v1/iot-service/api/user/profile",
        body: Body::None,
    }
}

// ---------------------------------------------------------------- devices

pub fn devices() -> Call {
    Call {
        name: "GET /v1/iot-service/api/user/device/list",
        method: "GET",
        path: "/v1/iot-service/api/user/device/list",
        body: Body::None,
    }
}

/// One printer's state as the cloud sees it. The fields Bambu leaves out when a
/// machine is offline are the reason everything here is optional.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CloudDevice {
    #[serde(default)]
    pub dev_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub dev_model_name: String,
    #[serde(default)]
    pub dev_product_name: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub print_status: Option<String>,
    #[serde(default)]
    pub mc_print_stage: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub mc_percent: Option<i64>,
    #[serde(default)]
    pub mc_remaining_time: Option<i64>,
}

impl CloudDevice {
    /// A line a person can read: the machine's name, or its model when Bambu
    /// has not given it one.
    pub fn label(&self) -> String {
        if !self.name.trim().is_empty() {
            self.name.clone()
        } else if !self.dev_product_name.trim().is_empty() {
            self.dev_product_name.clone()
        } else {
            self.dev_id.clone()
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeviceList {
    #[serde(default)]
    pub devices: Vec<CloudDevice>,
}

/// Create a print task: one multipart body, a JSON `task` part and the file.
/// The file goes up as `plate.gcode` whatever its name on disk, because the
/// cloud keys the task on the part's filename and not on anything in the body.
pub fn create_task(dev_id: &str, task_name: &str, file_name: &str, file: &[u8]) -> Call {
    let boundary = format!("----shale{:x}", super::now_unix());
    let task = serde_json::json!({
        "dev_id": dev_id,
        "task_name": task_name,
        "project_id": "0",
        "mode": "cloud_file",
    });
    let mut bytes = Vec::new();
    let mut part = |name: &str, filename: Option<&str>, content_type: &str, body: &[u8]| {
        bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(filename) => bytes.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => bytes.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\r\n");
    };
    part(
        "task",
        None,
        "application/json",
        task.to_string().as_bytes(),
    );
    part("file", Some(file_name), "application/octet-stream", file);
    bytes.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    Call {
        name: "POST /v1/iot-service/api/user/task",
        method: "POST",
        path: "/v1/iot-service/api/user/task",
        body: Body::Multipart { boundary, bytes },
    }
}

// ---------------------------------------------------------------- answers

/// The answer to a sign-in. Bambu says `loginType: verifyCode` when it wants a
/// code, and hands back tokens otherwise; the same struct reads both, because
/// the fields are optional in both cases.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginAnswer {
    #[serde(default)]
    pub login_type: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub refresh_expires_in: Option<i64>,
    #[serde(default)]
    pub tfa_key: Option<String>,
    #[serde(default)]
    pub access_method: Option<String>,
}

impl LoginAnswer {
    /// Bambu sometimes answers 200 with a complaint in the body; this is where
    /// that becomes an error instead of a silent empty token.
    pub fn tokens(&self) -> Option<(&str, &str)> {
        match (self.access_token.as_deref(), self.refresh_token.as_deref()) {
            (Some(access), Some(refresh)) if !access.is_empty() && !refresh.is_empty() => {
                Some((access, refresh))
            }
            _ => None,
        }
    }

    pub fn wants_code(&self) -> bool {
        self.login_type.as_deref() == Some("verifyCode") && self.tfa_key.is_some()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskCreated {
    #[serde(default)]
    pub id: Option<serde_json::Value>,
}

impl TaskCreated {
    pub fn id(&self) -> Option<String> {
        match self.id.as_ref()? {
            serde_json::Value::Number(number) => Some(number.to_string()),
            serde_json::Value::String(text) if !text.is_empty() => Some(text.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_call_names_itself_and_carries_the_password_only_in_the_body() {
        let call = login("a@b.c", "hunter2");
        let request = call.into_request(GLOBAL_HOST, None);
        assert_eq!(request.method, "POST");
        assert_eq!(
            request.url,
            "https://api.bambulab.com/v1/user-service/user/login"
        );
        assert!(request.name.contains("/login"));
        let body = String::from_utf8(request.body.unwrap()).unwrap();
        assert!(body.contains("hunter2"));
        assert!(!request.url.contains("hunter2"));
        assert!(request
            .headers
            .iter()
            .all(|(name, _)| name != "Authorization"));
    }

    #[test]
    fn a_code_answer_is_read_as_wanting_a_code() {
        let answer: LoginAnswer = serde_json::from_str(
            r#"{"loginType":"verifyCode","tfaKey":"abc","accessToken":"","refreshToken":""}"#,
        )
        .unwrap();
        assert!(answer.wants_code());
        assert!(answer.tokens().is_none());
    }

    #[test]
    fn a_token_answer_is_read_as_tokens() {
        let answer: LoginAnswer =
            serde_json::from_str(r#"{"accessToken":"a","refreshToken":"r","expiresIn":86400}"#)
                .unwrap();
        assert!(!answer.wants_code());
        assert_eq!(answer.tokens(), Some(("a", "r")));
    }

    #[test]
    fn a_device_without_a_name_falls_back_to_its_model_then_its_id() {
        let mut device = CloudDevice {
            dev_id: "01".into(),
            ..Default::default()
        };
        assert_eq!(device.label(), "01");
        device.dev_product_name = "A1".into();
        assert_eq!(device.label(), "A1");
        device.name = "workshop".into();
        assert_eq!(device.label(), "workshop");
    }

    #[test]
    fn a_task_body_carries_the_file_and_the_device() {
        let call = create_task("01P00", "cube", "plate.gcode", b"G28\n");
        let request = call.into_request(GLOBAL_HOST, Some("token"));
        assert_eq!(
            request.url,
            "https://api.bambulab.com/v1/iot-service/api/user/task"
        );
        assert!(request
            .headers
            .iter()
            .any(|(n, v)| n == "Authorization" && v == "Bearer token"));
        let body = String::from_utf8(request.body.unwrap()).unwrap();
        assert!(body.contains("\"dev_id\":\"01P00\""));
        assert!(body.contains("filename=\"plate.gcode\""));
        assert!(body.contains("G28\n"));
    }

    #[test]
    fn a_ticket_exchange_keeps_the_ticket_out_of_the_call_name() {
        let ticket = "ticket-secret-value";
        let request = ticket_exchange_request(GLOBAL_HOST, ticket);
        assert_eq!(request.name, "POST /v1/user-service/user/ticket");
        assert!(request.url.contains(ticket));
        assert!(!request.name.contains(ticket));
        let body = br#"{"accessToken":"access-secret","refreshToken":"refresh-secret","expiresIn":3600}"#;
        let (access, refresh, expires) = tokens_from_ticket_body(body).unwrap();
        assert_eq!(access, "access-secret");
        assert_eq!(refresh, "refresh-secret");
        assert_eq!(expires, 3600);
        let wrapped = br#"{"code":0,"data":{"accessToken":"access-secret","refreshToken":"refresh-secret","expiresIn":"120"}}"#;
        assert_eq!(tokens_from_ticket_body(wrapped).unwrap().2, 120);
        let profile = br#"{"data":{"account":"a@b.c","name":"Ada"}}"#;
        assert_eq!(account_from_profile(profile).as_deref(), Some("a@b.c"));
    }
}
