//! Bambu Lab's own sign-in page, the way Bambu Studio opens it.
//!
//! Studio loads `https://bambulab.com/en/sign-in` and answers the page's
//! `get_localhost_url` message with a loopback address. The page (and Google
//! or Apple, when the page asks for them) comes back to
//! `http://127.0.0.1:<port>/login?ticket=…`. This module does that exchange
//! and stores the tokens with the same vault call as the email form.
//!
//! The ticket and the tokens are never written to a log.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use shale_host::cloud::{api, Transport};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

enum LoginEvent {
    Bridge(String),
    Ticket { ticket: String },
}

pub(crate) enum BridgeAction {
    Localhost { sequence: Value },
    Navigate(String),
    Ticket(String),
    Tokens { access: String, refresh: String, expires_in: i64 },
}

pub async fn sign_in(app: &AppHandle, host: &str) -> Result<(String, String, i64, String), String> {
    let (tx, rx) = mpsc::channel();
    let alive = Arc::new(AtomicBool::new(true));
    let port = listen(tx.clone(), Arc::clone(&alive))?;
    let page = sign_in_page(host);
    let window = open_window(app, &page, port, tx)?;
    let host = host.to_string();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || wait(app, window, rx, alive, &host, port))
        .await
        .map_err(|err| err.to_string())?
}

fn wait(
    app: AppHandle,
    window: tauri::WebviewWindow,
    rx: Receiver<LoginEvent>,
    alive: Arc<AtomicBool>,
    host: &str,
    port: u16,
) -> Result<(String, String, i64, String), String> {
    let started = Instant::now();
    let mut adopted: Option<(String, String, i64)> = None;
    let mut account = "Bambu account".to_string();
    let outcome: Result<(), String> = loop {
        if started.elapsed() > Duration::from_secs(600) {
            break Err("The Bambu Lab sign-in window was open for ten minutes without finishing.".into());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(LoginEvent::Bridge(payload)) => match interpret_bridge(&payload) {
                Some(BridgeAction::Localhost { sequence }) => {
                    let ack = serde_json::json!({
                        "command": "get_localhost_url",
                        "sequence_id": sequence,
                        "response": {
                            "base_url": format!("http://127.0.0.1:{port}/"),
                            "result": "success"
                        }
                    });
                    let script = format!(
                        "window.postMessage({}, \"*\")",
                        ack
                    );
                    let _ = window.eval(&script);
                }
                Some(BridgeAction::Navigate(url)) => {
                    if let Ok(target) = url.parse::<Url>() {
                        let _ = window.navigate(target);
                    }
                }
                Some(BridgeAction::Ticket(ticket)) => {
                    if let Some(done) = exchange(host, &ticket) {
                        account = done.3;
                        adopted = Some((done.0, done.1, done.2));
                        break Ok(());
                    }
                }
                Some(BridgeAction::Tokens { access, refresh, expires_in }) => {
                    adopted = Some((access, refresh, expires_in));
                    break Ok(());
                }
                None => {}
            },
            Ok(LoginEvent::Ticket { ticket }) => {
                if let Some(done) = exchange(host, &ticket) {
                    account = done.3;
                    adopted = Some((done.0, done.1, done.2));
                    break Ok(());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if app.get_webview_window("bambu-login").is_none() {
                    break Err("The sign-in window was closed before Bambu Lab finished.".into());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err("The sign-in listener stopped.".into());
            }
        }
    };
    alive.store(false, Ordering::SeqCst);
    let _ = window.close();
    outcome?;
    let (access, refresh, expires) = adopted.ok_or_else(|| "Bambu Lab did not return a sign-in.".to_string())?;
    Ok((access, refresh, expires, account))
}

fn exchange(host: &str, ticket: &str) -> Option<(String, String, i64, String)> {
    let transport = shale_host::cloud::HttpTransport;
    let first = transport.send(api::ticket_exchange_request(host, ticket)).ok()?;
    let response = if (200..300).contains(&first.status) {
        first
    } else {
        let second = transport.send(api::ticket_exchange_body_request(host, ticket)).ok()?;
        if !(200..300).contains(&second.status) {
            return None;
        }
        second
    };
    let (access, refresh, expires) = api::tokens_from_ticket_body(&response.body)?;
    let account = transport
        .send(api::my_profile_request(host, &access))
        .ok()
        .filter(|profile| (200..300).contains(&profile.status))
        .and_then(|profile| api::account_from_profile(&profile.body))
        .unwrap_or_else(|| "Bambu account".into());
    Some((access, refresh, expires, account))
}

fn sign_in_page(host: &str) -> String {
    if host.contains("bambulab.cn") {
        "https://bambulab.cn/zh/sign-in".into()
    } else {
        "https://bambulab.com/en/sign-in".into()
    }
}

fn open_window(app: &AppHandle, page: &str, port: u16, tx: Sender<LoginEvent>) -> Result<tauri::WebviewWindow, String> {
    if let Some(existing) = app.get_webview_window("bambu-login") {
        let _ = existing.close();
    }
    let url: Url = page.parse().map_err(|err| format!("{err}"))?;
    let script = format!(
        r#"(function(){{
  var send = function(payload) {{
    var text = typeof payload === "string" ? payload : JSON.stringify(payload);
    location.assign("http://127.0.0.1:{port}/bridge?payload=" + encodeURIComponent(text));
  }};
  window.wx = window.wx || {{}};
  window.wx.postMessage = send;
  var chrome = (window.chrome = window.chrome || {{}});
  var webview = (chrome.webview = chrome.webview || {{}});
  webview.postMessage = send;
  webview.addEventListener = webview.addEventListener || function() {{}};
}})();"#
    );
    WebviewWindowBuilder::new(app, "bambu-login", WebviewUrl::External(url))
        .title("Sign in to Bambu Lab")
        .inner_size(480.0, 720.0)
        .user_agent("BBL-Slicer/v02.02.00.00")
        .initialization_script(&script)
        .on_navigation(move |url| {
            if url.host_str() == Some("127.0.0.1") && url.path() == "/bridge" {
                if let Some((_, payload)) = url.query_pairs().find(|(key, _)| key == "payload") {
                    let _ = tx.send(LoginEvent::Bridge(payload.into_owned()));
                }
                return false;
            }
            true
        })
        .build()
        .map_err(|err| err.to_string())
}

fn listen(tx: Sender<LoginEvent>, alive: Arc<AtomicBool>) -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|err| err.to_string())?;
    listener.set_nonblocking(true).map_err(|err| err.to_string())?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    std::thread::spawn(move || {
        while alive.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buf = [0u8; 8192];
                    let Ok(read) = stream.read(&mut buf) else {
                        continue;
                    };
                    let text = String::from_utf8_lossy(&buf[..read]);
                    let line = text.lines().next().unwrap_or("");
                    if let Some(ticket) = query_param(line, "ticket") {
                        let redirect = query_param(line, "redirect_url");
                        let location = safe_redirect(redirect.as_deref());
                        let _ = tx.send(LoginEvent::Ticket { ticket });
                        let header = format!(
                            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        let _ = stream.write_all(header.as_bytes());
                    } else {
                        let _ = stream.write_all(
                            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(40));
                }
                Err(_) => break,
            }
        }
    });
    Ok(port)
}

fn safe_redirect(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return "https://bambulab.com/en/sign-in".into();
    };
    let Ok(url) = raw.parse::<Url>() else {
        return "https://bambulab.com/en/sign-in".into();
    };
    let host = url.host_str().unwrap_or("");
    let allowed = host == "bambulab.com"
        || host.ends_with(".bambulab.com")
        || host == "bambulab.cn"
        || host.ends_with(".bambulab.cn");
    if url.scheme() == "https" && allowed {
        raw.to_string()
    } else {
        "https://bambulab.com/en/sign-in".into()
    }
}

pub(crate) fn interpret_bridge(payload: &str) -> Option<BridgeAction> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let command = value.get("command").and_then(|item| item.as_str()).unwrap_or("");
    match command {
        "get_localhost_url" => Some(BridgeAction::Localhost {
            sequence: value.get("sequence_id").cloned().unwrap_or(Value::Null),
        }),
        "thirdparty_login" | "new_webpage" => {
            let url = value
                .pointer("/data/url")
                .or_else(|| value.pointer("/data"))
                .and_then(|item| item.as_str())?;
            if url.starts_with("https://") {
                Some(BridgeAction::Navigate(url.to_string()))
            } else {
                None
            }
        }
        "user_login" | "user_ticket_login" | "login" => {
            if let Some(ticket) = value
                .pointer("/data/ticket")
                .and_then(|item| item.as_str())
                .filter(|text| !text.is_empty())
            {
                return Some(BridgeAction::Ticket(ticket.to_string()));
            }
            api::tokens_from_value(&value).map(|(access, refresh, expires_in)| BridgeAction::Tokens {
                access,
                refresh,
                expires_in,
            })
        }
        _ => None,
    }
}

pub(crate) fn query_param(request_line: &str, key: &str) -> Option<String> {
    let path = request_line.split_whitespace().nth(1)?;
    let query = path.split_once('?')?.1.split_whitespace().next()?;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=')?;
        if name == key {
            return Some(percent_decode(value));
        }
    }
    None
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &raw[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bridge_message_is_read_without_keeping_the_ticket_in_the_command_name() {
        let payload = r#"{"command":"user_ticket_login","data":{"ticket":"ticket-secret"}}"#;
        match interpret_bridge(payload) {
            Some(BridgeAction::Ticket(ticket)) => assert_eq!(ticket, "ticket-secret"),
            other => panic!("expected a ticket, got {}", other.is_some()),
        }
        let line = "GET /login?ticket=ticket-secret&redirect_url=https%3A%2F%2Fbambulab.com%2Fen HTTP/1.1";
        assert_eq!(query_param(line, "ticket").as_deref(), Some("ticket-secret"));
        assert_eq!(
            query_param(line, "redirect_url").as_deref(),
            Some("https://bambulab.com/en")
        );
        assert_eq!(
            safe_redirect(Some("https://evil.example/steal")),
            "https://bambulab.com/en/sign-in"
        );
    }
}
