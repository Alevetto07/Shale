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
    pub body: Vec<u8>,
}

impl Response {
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

    let mut raw = Vec::new();
    reader
        .read_to_end(&mut raw)
        .map_err(|err| HttpError::Io(format!("cannot read the answer: {err}")))?;
    parse(&raw)
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
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
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

    Ok(Response { status, body })
}

/// GET a file, following a few redirects, and write the body as it arrives.
///
/// The model download uses this. A test points it at loopback. It does not
/// add a query or a body, and it does not retry a failed host.
pub fn download_to(
    start: &str,
    sink: &mut impl Write,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<u64, HttpError> {
    let mut url = start.to_string();
    for _ in 0..5 {
        let parsed = Url::parse(&url)?;
        let mut reader = connect(&parsed)?;
        let (status, headers) = write_get(&parsed, &mut reader)?;
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let location = header_value(&headers, "location").ok_or_else(|| {
                HttpError::Protocol("a redirect arrived with no location".into())
            })?;
            url = resolve_redirect(&parsed, location);
            continue;
        }
        if status != 200 {
            return Err(HttpError::Protocol(format!(
                "the download answered {status}"
            )));
        }
        let total = header_value(&headers, "content-length").and_then(|value| value.parse().ok());
        return copy_body(&mut reader, sink, total, &mut progress);
    }
    Err(HttpError::Protocol(
        "the download redirected more than five times".into(),
    ))
}

fn connect(url: &Url) -> Result<Box<dyn ReadWrite>, HttpError> {
    let timeout = Duration::from_secs(120);
    let stream = TcpStream::connect((url.host.as_str(), url.port))
        .map_err(|err| HttpError::Io(format!("cannot reach {}: {err}", url.host)))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| HttpError::Io(err.to_string()))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| HttpError::Io(err.to_string()))?;
    if !url.secure {
        return Ok(Box::new(stream));
    }
    let name = ServerName::try_from(url.host.clone())
        .map_err(|_| HttpError::Url(format!("`{}` is not a server name", url.host)))?;
    let connection = ClientConnection::new(std::sync::Arc::new(tls_config()), name)
        .map_err(|err| HttpError::Io(format!("TLS: {err}")))?;
    Ok(Box::new(StreamOwned::new(connection, stream)))
}

fn write_get(url: &Url, reader: &mut dyn ReadWrite) -> Result<(u16, Vec<(String, String)>), HttpError> {
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Shale/0.1\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        url.path,
        url.host_header()
    );
    reader
        .write_all(request.as_bytes())
        .map_err(|err| HttpError::Io(format!("cannot write the request: {err}")))?;
    reader
        .flush()
        .map_err(|err| HttpError::Io(format!("cannot flush the request: {err}")))?;
    read_header(reader)
}

fn read_header(reader: &mut dyn Read) -> Result<(u16, Vec<(String, String)>), HttpError> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = reader
            .read(&mut byte)
            .map_err(|err| HttpError::Io(format!("cannot read the answer: {err}")))?;
        if n == 0 {
            return Err(HttpError::Protocol(
                "the answer closed before the header ended".into(),
            ));
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
        if buf.len() > 65_536 {
            return Err(HttpError::Protocol("the header is too long".into()));
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| HttpError::Protocol(format!("`{status_line}` is not a status line")))?;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    Ok((status, headers))
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn resolve_redirect(from: &Url, location: &str) -> String {
    if location.starts_with("https://") || location.starts_with("http://") {
        return location.to_string();
    }
    let scheme = if from.secure { "https" } else { "http" };
    let path = if location.starts_with('/') {
        location.to_string()
    } else {
        format!("/{}", location.trim_start_matches('/'))
    };
    let default = if from.secure { 443 } else { 80 };
    if from.port == default {
        format!("{scheme}://{}{path}", from.host)
    } else {
        format!("{scheme}://{}:{}{path}", from.host, from.port)
    }
}

fn copy_body(
    reader: &mut dyn Read,
    sink: &mut impl Write,
    total: Option<u64>,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<u64, HttpError> {
    let mut received = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        if total.is_some_and(|total| received >= total) {
            break;
        }
        let n = reader
            .read(&mut buf)
            .map_err(|err| HttpError::Io(format!("cannot read the file: {err}")))?;
        if n == 0 {
            break;
        }
        sink.write_all(&buf[..n])
            .map_err(|err| HttpError::Io(format!("cannot write the file: {err}")))?;
        received += n as u64;
        progress(received, total);
    }
    Ok(received)
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
