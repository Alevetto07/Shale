//! The chamber camera.
//!
//! Two LAN protocols, chosen by the machine:
//!
//! * **A1, A1 mini, P1P, P1S** — a TLS socket on port **6000**. An 80-byte
//!   authentication packet (`bblp` and the LAN access code), then a 16-byte
//!   header and a JPEG. This is the stream OpenBambuAPI documents for the A1
//!   and the P1 series.
//! * **P2S, X1, H2** — RTSPS on port **322**,
//!   `rtsps://<ip>:322/streaming/live/1`, user `bblp`, the same access code.
//!   OpenBambuAPI and the P2S bridge notes agree: the P2S does **not** speak
//!   the port-6000 JPEG protocol. The picture is H.264. Shale decodes it with
//!   OpenH264, built from source, and stores a JPEG so the rest of the camera
//!   path is the same as the A1. LAN-only live view has to be on, or the port
//!   closes immediately.
//!
//! The access code is the credential. It is never written into an error, a
//! log line, or [`Debug`] of a target.
//!
//! Nothing here starts a print or jogs an axis. A frame is a picture.

mod cloud;
mod detect;
mod h264;
mod jpeg;
mod obico;
mod plate;
mod rtsps;
mod weights;

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::StreamOwned;

pub use cloud::{interpret_cloud_body, snapshot_request, CLOUD_CAMERA_PATH};
pub use detect::{
    evaluate, inspect, sample_cases, sample_frame, watch_action, BoxMark, Debounce, EvalCase,
    EvalReport, FailureKind, FrameFinding, Sensitivity, WatchSettings, OBICO_ONNX_BYTES,
    OBICO_ONNX_SHA256, OBICO_ONNX_URL,
};

/// Standard base64, so a snapshot can travel to the window and to MCP.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let n = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | bytes[index + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        index += 3;
    }
    if index < bytes.len() {
        let left = bytes.len() - index;
        let mut n = (bytes[index] as u32) << 16;
        if left == 2 {
            n |= (bytes[index + 1] as u32) << 8;
        }
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if left == 2 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
            out.push('=');
        } else {
            out.push('=');
            out.push('=');
        }
    }
    out
}
pub use jpeg::{auth_packet, read_header, JpegHeader};
pub use plate::{decode_jpeg, plates_match, PlateVerdict};
pub use h264::nal_payloads;
pub use obico::score as score_obico;
pub use rtsps::{depacketise_h264, encode_interleaved_rtp, encode_rtp};
pub use weights::{install as install_model, is_verified as model_is_verified, model_path, release_url, FILE_NAME as MODEL_FILE};

/// Which socket a model speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraPath {
    /// JPEG frames over TLS. Port 6000 on a printer.
    JpegTls,
    /// RTSPS. Port 322 on a printer.
    Rtsps,
}

impl CameraPath {
    pub fn default_port(self) -> u16 {
        match self {
            Self::JpegTls => 6000,
            Self::Rtsps => 322,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::JpegTls => "jpeg",
            Self::Rtsps => "rtsps",
        }
    }
}

/// A1 and P1 series use the JPEG socket. P2S and the X1/H2 family use RTSPS.
///
/// An unknown name stays on the JPEG socket: that is the one Shale can turn
/// into a picture without a video decoder, and a wrong guess fails closed
/// with a camera error rather than a move.
pub fn path_for_model(model: &str) -> CameraPath {
    let id = model.to_ascii_lowercase().replace([' ', '-'], "_");
    let rtsps = ["p2s", "n2s", "x1", "x2", "h2", "h3", "n7"];
    if rtsps.iter().any(|needle| id.contains(needle)) {
        return CameraPath::Rtsps;
    }
    CameraPath::JpegTls
}

/// Why a camera did not produce a picture. The text is what the panel shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraFault {
    WrongAccessCode,
    LanLiveViewOff,
    CameraDisabled,
    Unreachable,
    CloudUnavailable,
    Protocol(String),
}

impl CameraFault {
    pub fn code(&self) -> &'static str {
        match self {
            Self::WrongAccessCode => "wrong_access_code",
            Self::LanLiveViewOff => "lan_live_view_off",
            Self::CameraDisabled => "camera_disabled",
            Self::Unreachable => "unreachable",
            Self::CloudUnavailable => "cloud_unavailable",
            Self::Protocol(_) => "protocol",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::WrongAccessCode => {
                "The access code was refused. Check the code on the printer's screen.".into()
            }
            Self::LanLiveViewOff => {
                "LAN-only live view is off. On the printer, open the network settings and turn \
                 on LAN-only live view. The printer can stay signed in to the cloud."
                    .into()
            }
            Self::CameraDisabled => "The camera is switched off on the printer.".into(),
            Self::Unreachable => {
                "Shale could not reach the camera. Check the printer's address and that it is \
                 on this network."
                    .into()
            }
            Self::CloudUnavailable => {
                "The cloud camera is not available. Bambu's remote picture uses a private relay \
                 Shale does not speak. Use the LAN camera instead."
                    .into()
            }
            Self::Protocol(detail) => detail.clone(),
        }
    }
}

impl std::fmt::Display for CameraFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for CameraFault {}

/// Where the camera is. [`Debug`] names the host and the path, never the code.
#[derive(Clone)]
pub struct CameraTarget {
    pub host: String,
    pub port: u16,
    pub path: CameraPath,
    access_code: String,
}

impl std::fmt::Debug for CameraTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CameraTarget")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("path", &self.path.as_str())
            .field(
                "access_code",
                &if self.access_code.is_empty() {
                    "empty"
                } else {
                    "set"
                },
            )
            .finish()
    }
}

impl CameraTarget {
    pub fn new(host: impl Into<String>, model: &str, access_code: impl Into<String>) -> Self {
        let path = path_for_model(model);
        Self {
            host: host.into(),
            port: path.default_port(),
            path,
            access_code: access_code.into(),
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_path(mut self, path: CameraPath) -> Self {
        self.path = path;
        self
    }

    pub fn access_code(&self) -> &str {
        &self.access_code
    }
}

/// One picture, or an H.264 access unit when the P2S stream has not been
/// decoded to a JPEG. This build does not ship an H.264 decoder.
#[derive(Clone, Debug)]
pub enum Frame {
    Jpeg(Vec<u8>),
    H264(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct CameraStatus {
    pub running: bool,
    pub path: String,
    pub frames: u64,
    pub fps: f64,
    pub has_jpeg: bool,
    pub codec: String,
    pub fault_code: Option<String>,
    pub fault: Option<String>,
}

struct Shared {
    running: bool,
    path: String,
    frames: u64,
    fps: f64,
    jpeg: Option<Vec<u8>>,
    codec: String,
    fault: Option<CameraFault>,
    started: Instant,
}

/// A camera thread. Dropping it, or [`CameraSession::stop`], ends the socket.
/// It never publishes a motion command.
pub struct CameraSession {
    stop: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
    thread: Option<JoinHandle<()>>,
}

impl CameraSession {
    pub fn start(target: CameraTarget) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared {
            running: true,
            path: target.path.as_str().to_string(),
            frames: 0,
            fps: 0.0,
            jpeg: None,
            codec: String::new(),
            fault: None,
            started: Instant::now(),
        }));
        let stop_thread = Arc::clone(&stop);
        let shared_thread = Arc::clone(&shared);
        let thread = thread::spawn(move || drive(target, stop_thread, shared_thread));
        Self {
            stop,
            shared,
            thread: Some(thread),
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn status(&self) -> CameraStatus {
        let shared = self.shared.lock().unwrap();
        CameraStatus {
            running: shared.running,
            path: shared.path.clone(),
            frames: shared.frames,
            fps: shared.fps,
            has_jpeg: shared.jpeg.is_some(),
            codec: shared.codec.clone(),
            fault_code: shared
                .fault
                .as_ref()
                .map(CameraFault::code)
                .map(str::to_string),
            fault: shared.fault.as_ref().map(CameraFault::message),
        }
    }

    pub fn latest_jpeg(&self) -> Option<Vec<u8>> {
        self.shared.lock().unwrap().jpeg.clone()
    }
}

impl Drop for CameraSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn drive(target: CameraTarget, stop: Arc<AtomicBool>, shared: Arc<Mutex<Shared>>) {
    let mut pictures = h264::H264Pictures::new().ok();
    let outcome = match target.path {
        CameraPath::JpegTls => jpeg::stream(&target, &stop, |frame| note_frame(&shared, frame)),
        CameraPath::Rtsps => rtsps::stream(&target, &stop, |frame| {
            note_stream_frame(&shared, &mut pictures, frame)
        }),
    };
    let mut shared = shared.lock().unwrap();
    shared.running = false;
    if let Err(fault) = outcome {
        if shared.fault.is_none() {
            shared.fault = Some(fault);
        }
    }
}

fn note_frame(shared: &Mutex<Shared>, frame: Frame) -> bool {
    let mut shared = shared.lock().unwrap();
    shared.frames += 1;
    let elapsed = shared.started.elapsed().as_secs_f64().max(0.05);
    shared.fps = shared.frames as f64 / elapsed;
    match frame {
        Frame::Jpeg(bytes) => {
            shared.codec = "jpeg".into();
            shared.jpeg = Some(bytes);
        }
        Frame::H264(_) => {
            if shared.codec.is_empty() {
                shared.codec = "h264".into();
            }
        }
    }
    true
}

fn note_stream_frame(
    shared: &Mutex<Shared>,
    pictures: &mut Option<h264::H264Pictures>,
    frame: Frame,
) -> bool {
    match frame {
        Frame::Jpeg(bytes) => note_frame(shared, Frame::Jpeg(bytes)),
        Frame::H264(unit) => {
            let still = pictures.as_mut().and_then(|decoder| decoder.push(&unit));
            if let Some(jpeg) = still {
                let mut shared = shared.lock().unwrap();
                shared.frames += 1;
                let elapsed = shared.started.elapsed().as_secs_f64().max(0.05);
                shared.fps = shared.frames as f64 / elapsed;
                shared.codec = "h264".into();
                shared.jpeg = Some(jpeg);
            }
            true
        }
    }
}

pub(crate) fn connect_tls(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<StreamOwned<rustls::ClientConnection, TcpStream>, CameraFault> {
    let trust = crate::Trust::new(None);
    let addr = (host, port)
        .to_socket_addrs_timeout(timeout)
        .map_err(|_| CameraFault::Unreachable)?;
    let sock = TcpStream::connect_timeout(&addr, timeout).map_err(classify_connect)?;
    sock.set_nodelay(true).ok();
    sock.set_read_timeout(Some(timeout)).ok();
    sock.set_write_timeout(Some(timeout)).ok();
    let name = ServerName::try_from(host.to_string())
        .map_err(|_| CameraFault::Protocol(format!("{host} is not a name the camera can use")))?;
    let conn = rustls::ClientConnection::new(trust.client_config(), name)
        .map_err(|err| CameraFault::Protocol(err.to_string()))?;
    let mut stream = StreamOwned::new(conn, sock);
    stream
        .conn
        .complete_io(&mut stream.sock)
        .map_err(classify_handshake)?;
    Ok(stream)
}

trait ToSocket {
    fn to_socket_addrs_timeout(&self, timeout: Duration) -> io::Result<std::net::SocketAddr>;
}

impl ToSocket for (&str, u16) {
    fn to_socket_addrs_timeout(&self, _timeout: Duration) -> io::Result<std::net::SocketAddr> {
        let mut addrs = (*self).to_socket_addrs()?;
        addrs
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address"))
    }
}

fn classify_connect(err: io::Error) -> CameraFault {
    match err.kind() {
        io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut | io::ErrorKind::NotFound => {
            CameraFault::Unreachable
        }
        io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof => {
            CameraFault::LanLiveViewOff
        }
        _ => {
            let text = err.to_string();
            if text.contains("reset") || text.contains("closed") {
                CameraFault::LanLiveViewOff
            } else {
                CameraFault::Unreachable
            }
        }
    }
}

fn classify_handshake(err: io::Error) -> CameraFault {
    // A printer with LAN-only live view off accepts the TCP connection and
    // then drops it, which is what OpenBambuAPI records for port 322.
    let text = err.to_string();
    if text.contains("reset")
        || text.contains("closed")
        || text.contains("eof")
        || err.kind() == io::ErrorKind::UnexpectedEof
        || err.kind() == io::ErrorKind::ConnectionReset
    {
        CameraFault::LanLiveViewOff
    } else {
        CameraFault::Protocol(format!("the camera's TLS handshake failed ({text})"))
    }
}

pub(crate) fn read_exact(stream: &mut impl Read, buf: &mut [u8]) -> Result<(), CameraFault> {
    match stream.read_exact(buf) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Err(CameraFault::WrongAccessCode),
        Err(err)
            if err.kind() == io::ErrorKind::TimedOut || err.kind() == io::ErrorKind::WouldBlock =>
        {
            Err(CameraFault::Protocol(
                "the camera accepted the connection and then sent nothing".into(),
            ))
        }
        Err(err) => Err(classify_connect(err)),
    }
}

pub(crate) fn write_all(stream: &mut impl Write, bytes: &[u8]) -> Result<(), CameraFault> {
    stream.write_all(bytes).map_err(classify_connect)?;
    stream.flush().map_err(classify_connect)?;
    Ok(())
}

/// Read one JPEG from a port-6000 camera. Used by the snapshot tool and by tests.
pub fn snapshot_jpeg(target: &CameraTarget, timeout: Duration) -> Result<Vec<u8>, CameraFault> {
    match target.path {
        CameraPath::JpegTls => jpeg::one_frame(target, timeout),
        CameraPath::Rtsps => {
            let stop = AtomicBool::new(false);
            let mut jpeg = None;
            let mut saw_h264 = false;
            let mut pictures = h264::H264Pictures::new().ok();
            let result = rtsps::stream(target, &stop, |frame| match frame {
                Frame::Jpeg(bytes) => {
                    jpeg = Some(bytes);
                    false
                }
                Frame::H264(unit) => {
                    saw_h264 = true;
                    if let Some(still) = pictures.as_mut().and_then(|decoder| decoder.push(&unit)) {
                        jpeg = Some(still);
                        return false;
                    }
                    true
                }
            });
            if let Some(bytes) = jpeg {
                return Ok(bytes);
            }
            if saw_h264 {
                return Err(CameraFault::Protocol(
                    "The P2S camera is an H.264 stream over RTSPS. Shale reached it, but this \
                     frame did not decode into a picture."
                        .into(),
                ));
            }
            result?;
            Err(CameraFault::Protocol(
                "the RTSPS camera closed without a picture".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_a1_and_p1_use_jpeg_and_the_p2s_uses_rtsps() {
        assert_eq!(path_for_model("bambu_lab_a1"), CameraPath::JpegTls);
        assert_eq!(path_for_model("A1 mini"), CameraPath::JpegTls);
        assert_eq!(path_for_model("bambu_lab_p1s"), CameraPath::JpegTls);
        assert_eq!(path_for_model("bambu_lab_p1p"), CameraPath::JpegTls);
        assert_eq!(path_for_model("bambu_lab_p2s"), CameraPath::Rtsps);
        assert_eq!(path_for_model("P2S"), CameraPath::Rtsps);
        assert_eq!(path_for_model("X1 Carbon"), CameraPath::Rtsps);
        assert_eq!(path_for_model("H2D"), CameraPath::Rtsps);
        assert_eq!(CameraPath::JpegTls.default_port(), 6000);
        assert_eq!(CameraPath::Rtsps.default_port(), 322);
    }

    #[test]
    fn a_target_does_not_print_its_access_code() {
        let target = CameraTarget::new("192.168.1.20", "bambu_lab_a1", "SECRET-CODE");
        let text = format!("{target:?}");
        assert!(!text.contains("SECRET-CODE"));
        assert!(text.contains("set"));
        assert_eq!(target.access_code(), "SECRET-CODE");
    }

    #[test]
    fn the_three_camera_errors_say_what_to_do() {
        assert!(CameraFault::WrongAccessCode
            .message()
            .contains("access code"));
        assert!(CameraFault::LanLiveViewOff
            .message()
            .contains("LAN-only live view"));
        assert!(CameraFault::CameraDisabled
            .message()
            .contains("switched off"));
        assert_eq!(CameraFault::WrongAccessCode.code(), "wrong_access_code");
        assert!(!CameraFault::CloudUnavailable
            .message()
            .to_ascii_lowercase()
            .contains("deep field"));
    }
}
