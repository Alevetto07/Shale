//! The smallest HTTPS client that can talk to Bambu's cloud.
//!
//! This exists instead of a full HTTP stack because Shale needs three calls:
//! POST JSON, GET JSON, and one multipart upload. `reqwest` 0.13 brings a
//! platform verifier, sixteen crates and a C build of aws-lc with it; `ureq`
//! brings its own agent and redirect policy. Both are more than this needs, and
//! both are harder to reason about on the UNO Q than 300 lines that only do
//! what is written here.
//!
//! What is supported: https (and plain http for the loopback tests), one
//! request per connection, `Content-Length` and `chunked` responses, and no
//! redirects — a redirect from an API endpoint is a change in the API, and it
//! is reported rather than followed.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Once;
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

/// A connection that can be read and written, which is all a stream needs to
/// be. `Box<dyn Read>` alone loses `write`, and a request has to be sent.
trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

/// What went wrong before there is an HTTP status to report.
#[derive(Debug)]
pub enum HttpError {
    /// The URL could not be read as a URL.
    Url(String),
    /// The connection, the TLS handshake or the write failed.
    Io(String),
    /// The server answered something that is not HTTP/1.1.
    Protocol(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Url(message) | Self::Io(message) | Self::Protocol(message) => {
                f.write_str(message)
            }
        }
    }
}

impl std::error::Error for HttpError {}

/// A URL split into the pieces a client needs. Only what this client uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Url {
    pub fn parse(raw: &str) -> Result<Self, HttpError> {
        let (secure, rest) = if let Some(rest) = raw.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = raw.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err(HttpError::Url(format!(
                "`{raw}` is not an http or https URL"
            )));
        };
        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return Err(HttpError::Url(format!("`{raw}` has no host")));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
                let port = port.parse().map_err(|_| {
                    HttpError::Url(format!("`{raw}` has a port that is not a number"))
                })?;
                (host.to_string(), port)
            }
            _ => (authority.to_string(), if secure { 443 } else { 80 }),
        };
        Ok(Self {
            secure,
            host,
            port,
            path: path.to_string(),
        })
    }

    fn host_header(&self) -> String {
        let default = if self.secure { 443 } else { 80 };
        if self.port == default {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// What came back.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    /// Header names are stored in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn from_parts(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// One header, matched without case. Names are stored in lower case.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| key == &name)
            .map(|(_, value)| value.as_str())
    }

    /// The body as JSON, or an error that names the status — a cloud that
    /// answers HTML is a cloud that changed, and the status is the first clue.
    pub fn json(&self) -> Result<serde_json::Value, HttpError> {
        serde_json::from_slice(&self.body).map_err(|err| {
            HttpError::Protocol(format!(
                "the server answered {} with something that is not JSON ({err}); first bytes: {}",
                self.status,
                String::from_utf8_lossy(&self.body[..self.body.len().min(120)])
            ))
        })
    }
}

static INSTALL_PROVIDER: Once = Once::new();

fn tls_config() -> ClientConfig {
    // rustls 0.23 refuses to guess a crypto provider when more than one is
    // compiled in, and a library should not install a process-wide default
    // behind its caller's back. This installs ring once, and ignores the error
    // when somebody else got there first.
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// One request, one connection. `headers` are added to the four this sets, and
/// they carry the content type: this function does not guess one.
pub fn send(
    method: &str,
    url: &str,
    headers: &[(&str, String)],
    body: Option<Vec<u8>>,
) -> Result<Response, HttpError> {
    send_limited(method, url, headers, body, None)
}

/// [`send`], but the read stops once `max_bytes` have arrived. A model file
/// has a ceiling; an API that starts streaming a gigabyte should not be able
/// to fill memory before anybody looks at the status.
pub fn send_limited(
    method: &str,
    url: &str,
    headers: &[(&str, String)],
    body: Option<Vec<u8>>,
    max_bytes: Option<usize>,
) -> Result<Response, HttpError> {
    let url = Url::parse(url)?;
    let timeout = Duration::from_secs(30);
    let stream = TcpStream::connect((url.host.as_str(), url.port))
        .map_err(|err| HttpError::Io(format!("cannot reach {}: {err}", url.host)))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| HttpError::Io(err.to_string()))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| HttpError::Io(err.to_string()))?;

    let mut request = Vec::new();
    request.extend_from_slice(format!("{method} {} HTTP/1.1\r\n", url.path).as_bytes());
    request.extend_from_slice(format!("Host: {}\r\n", url.host_header()).as_bytes());
    request.extend_from_slice(
        b"User-Agent: Shale/0.1\r\nAccept: application/json\r\nConnection: close\r\n",
    );
    for (name, value) in headers {
        request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    if let Some(bytes) = &body {
        request.extend_from_slice(format!("Content-Length: {}\r\n", bytes.len()).as_bytes());
    }
    request.extend_from_slice(b"\r\n");
    if let Some(bytes) = &body {
        request.extend_from_slice(bytes);
    }

    let mut reader: Box<dyn ReadWrite> = if url.secure {
        let name = ServerName::try_from(url.host.clone())
            .map_err(|_| HttpError::Url(format!("`{}` is not a server name", url.host)))?;
        let connection = ClientConnection::new(std::sync::Arc::new(tls_config()), name)
            .map_err(|err| HttpError::Io(format!("TLS: {err}")))?;
        Box::new(StreamOwned::new(connection, stream))
    } else {
        Box::new(stream)
    };

    reader
        .write_all(&request)
        .map_err(|err| HttpError::Io(format!("cannot write the request: {err}")))?;
    reader
        .flush()
        .map_err(|err| HttpError::Io(format!("cannot flush the request: {err}")))?;

    let raw = read_limited(&mut reader, max_bytes)?;
    parse(&raw)
}

fn read_limited(
    reader: &mut dyn ReadWrite,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, HttpError> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buf)
            .map_err(|err| HttpError::Io(format!("cannot read the answer: {err}")))?;
        if read == 0 {
            break;
        }
        if let Some(max) = max_bytes {
            if raw.len().saturating_add(read) > max {
                return Err(HttpError::Protocol(format!(
                    "the answer is larger than {max} bytes"
                )));
            }
        }
        raw.extend_from_slice(&buf[..read]);
    }
    Ok(raw)
}

fn parse(raw: &[u8]) -> Result<Response, HttpError> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| HttpError::Protocol("the answer has no header block".into()))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut body = raw[split + 4..].to_vec();

    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| HttpError::Protocol(format!("`{status_line}` is not a status line")))?;

    let mut content_length = None;
    let mut chunked = false;
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        headers.push((name.clone(), value.to_string()));
        match name.as_str() {
            "content-length" => content_length = value.parse::<usize>().ok(),
            "transfer-encoding" if value.eq_ignore_ascii_case("chunked") => chunked = true,
            _ => {}
        }
    }

    if chunked {
        body = dechunk(&body)?;
    } else if let Some(length) = content_length {
        body.truncate(length);
    }

    Ok(Response {
        status,
        headers,
        body,
    })
}

fn dechunk(mut raw: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut out = Vec::new();
    while let Some(end) = raw.windows(2).position(|window| window == b"\r\n") {
        let size_line = String::from_utf8_lossy(&raw[..end]).to_string();
        let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("0").trim(), 16)
            .map_err(|_| HttpError::Protocol(format!("`{size_line}` is not a chunk size")))?;
        raw = &raw[end + 2..];
        if size == 0 {
            break;
        }
        if raw.len() < size {
            return Err(HttpError::Protocol(
                "a chunk is shorter than it says".into(),
            ));
        }
        out.extend_from_slice(&raw[..size]);
        raw = &raw[(size + 2).min(raw.len())..];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_keeps_its_parts() {
        let url = Url::parse("https://api.bambulab.com/v1/user-service/user/login").unwrap();
        assert!(url.secure);
        assert_eq!(url.host, "api.bambulab.com");
        assert_eq!(url.port, 443);
        assert_eq!(url.path, "/v1/user-service/user/login");
    }

    #[test]
    fn a_loopback_url_keeps_its_port() {
        let url = Url::parse("http://127.0.0.1:8765/health").unwrap();
        assert!(!url.secure);
        assert_eq!(url.port, 8765);
        assert_eq!(url.host_header(), "127.0.0.1:8765");
    }

    #[test]
    fn a_url_without_a_scheme_is_refused_by_name() {
        let err = Url::parse("api.bambulab.com/login").unwrap_err();
        assert!(
            err.to_string().contains("not an http or https URL"),
            "{err}"
        );
    }

    #[test]
    fn a_content_length_answer_is_cut_to_its_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{\"a\":1}";
        let response = parse(raw).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{\"");
    }

    #[test]
    fn a_location_header_is_kept() {
        let raw = b"HTTP/1.1 302 Found\r\nLocation: https://cdn.example/file.3mf\r\nContent-Length: 0\r\n\r\n";
        let response = parse(raw).unwrap();
        assert_eq!(response.status, 302);
        assert_eq!(
            response.header("location"),
            Some("https://cdn.example/file.3mf")
        );
    }

    #[test]
    fn a_chunked_answer_is_reassembled() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        let response = parse(raw).unwrap();
        assert_eq!(response.body, b"{\"a\":1}");
    }

    #[test]
    fn something_that_is_not_http_says_so() {
        let err = parse(b"<html>").unwrap_err();
        assert!(err.to_string().contains("no header block"), "{err}");
    }
}
