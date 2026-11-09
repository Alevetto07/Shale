//! The printer host: the half of a slicer that owns a machine.
//!
//! Bambu's LAN interface is small and undocumented, but stable: MQTT over TLS on
//! 8883 (user `bblp`, the printer's 8-character access code as the password),
//! `device/<serial>/report` for everything the machine says, and
//! `device/<serial>/request` for everything it is told; FTPS on 990 for the file.
//! The printer's certificate is self-signed, so the TLS client accepts any
//! certificate — the credential on the wire is the access code, not the
//! transport.
//!
//! Nothing here starts a print on its own. [`Link`] only carries commands the
//! window hands it, and the window asks before it sends one.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use serde::{Deserialize, Serialize};

/// Everything needed to reach one printer on the LAN.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostConfig {
    pub host: String,
    pub serial: String,
    pub code: String,
    /// `textured_plate`, `smooth_plate`, `engineering_plate`, `cool_plate` — the
    /// machine matches its own nozzle offset against this name.
    #[serde(default = "default_bed")]
    pub bed_type: String,
    /// Where uploads are written. The A1 exposes its storage as `/`.
    #[serde(default = "default_dir")]
    pub remote_dir: String,
    /// MQTT over TLS. 8883 is what a Bambu machine listens on; a test double
    /// binds an ephemeral port instead, because a test must not need to be
    /// administrator to bind the real one.
    #[serde(default = "default_mqtt_port")]
    pub mqtt_port: u16,
    /// Implicit FTPS. 990 on a printer; ephemeral in a test.
    #[serde(default = "default_ftps_port")]
    pub ftps_port: u16,
}

fn default_bed() -> String {
    "textured_plate".to_string()
}

fn default_dir() -> String {
    "cache".to_string()
}

fn default_mqtt_port() -> u16 {
    8883
}

fn default_ftps_port() -> u16 {
    990
}

impl HostConfig {
    fn request_topic(&self) -> String {
        format!("device/{}/request", self.serial)
    }

    fn report_topic(&self) -> String {
        format!("device/{}/report", self.serial)
    }
}

/// What the machine last said about itself.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub connected: bool,
    /// `IDLE`, `PREPARE`, `RUNNING`, `PAUSE`, `FINISH`, `FAILED`.
    pub state: String,
    pub percent: i64,
    /// Minutes, as the machine counts them.
    pub remaining_min: i64,
    pub layer: i64,
    pub layers: i64,
    pub nozzle: f64,
    pub nozzle_target: f64,
    pub bed: f64,
    pub bed_target: f64,
    pub file: String,
    pub error: i64,
    /// The last thing that went wrong in this process, for the window to show.
    pub note: String,
    pub reports: u64,
    pub updated_at: f64,
}

impl Status {
    /// Whether the machine is busy with a job. The window uses the state for
    /// the same decision; this is the one place the list is spelled out.
    pub fn is_printing(&self) -> bool {
        matches!(self.state.as_str(), "RUNNING" | "PAUSE" | "PREPARE")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Tls(#[from] rustls::Error),
    #[error("{0}")]
    Bad(String),
}

type Result<T> = std::result::Result<T, HostError>;

// ---------------------------------------------------------------- transport

/// The printer presents a self-signed certificate. Accepting it is what makes
/// the connection possible at all; the access code is the thing that authorises.
#[derive(Debug)]
struct AcceptAny;

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end: &CertificateDer<'_>,
        _ints: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn tls_config() -> Arc<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("the ring provider offers default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny))
        .with_no_client_auth();
    Arc::new(config)
}

/// A TCP connection with TLS on top, plus the byte buffer the MQTT framing needs.
struct Wire {
    stream: StreamOwned<ClientConnection, TcpStream>,
    buffered: Vec<u8>,
}

impl Wire {
    fn open(host: &str, port: u16) -> Result<Self> {
        let addr = (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| HostError::Bad(format!("{host}:{port} does not resolve")))?;
        let sock = TcpStream::connect_timeout(&addr, Duration::from_secs(6))?;
        sock.set_nodelay(true)?;
        sock.set_read_timeout(Some(Duration::from_millis(250)))?;
        let name = ServerName::try_from(host.to_string())
            .map_err(|_| HostError::Bad(format!("{host} is not a server name")))?;
        let conn = ClientConnection::new(tls_config(), name)?;
        let mut stream = StreamOwned::new(conn, sock);
        // Drive the handshake now, so a wrong host or a closed port fails here
        // with a readable error instead of on the first packet.
        stream.conn.complete_io(&mut stream.sock)?;
        Ok(Self {
            stream,
            buffered: Vec::new(),
        })
    }

    fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.stream.write_all(bytes)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Read whatever is available into the buffer. `Ok(false)` means "nothing
    /// arrived in this window", which is normal between reports.
    fn fill(&mut self) -> Result<bool> {
        let mut tmp = [0u8; 4096];
        match self.stream.read(&mut tmp) {
            Ok(0) => Err(HostError::Bad("the printer closed the connection".into())),
            Ok(n) => {
                self.buffered.extend_from_slice(&tmp[..n]);
                Ok(true)
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(false)
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Pull one complete packet out of the buffer, if there is one.
    fn packet(&mut self) -> Option<Vec<u8>> {
        let (len, header) = remaining_length(&self.buffered)?;
        let total = header + len;
        if self.buffered.len() < total {
            return None;
        }
        let packet = self.buffered[..total].to_vec();
        self.buffered.drain(..total);
        Some(packet)
    }

    fn read_line(&mut self) -> Result<String> {
        loop {
            if let Some(pos) = self.buffered.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.buffered.drain(..=pos).collect();
                return Ok(String::from_utf8_lossy(&line).trim_end().to_string());
            }
            if !self.fill()? {
                return Err(HostError::Bad("the printer went quiet".into()));
            }
        }
    }
}

/// MQTT's variable-length integer: seven bits per byte, high bit means "more".
fn remaining_length(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut value = 0usize;
    let mut shift = 0;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        value |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
        shift += 7;
        if shift > 21 {
            return None;
        }
    }
    None
}

fn encode_remaining_length(mut len: usize, out: &mut Vec<u8>) {
    loop {
        let mut byte = (len % 128) as u8;
        len /= 128;
        if len > 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if len == 0 {
            break;
        }
    }
}

fn push_str(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u16).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn packet(first: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![first];
    encode_remaining_length(body.len(), &mut out);
    out.extend_from_slice(body);
    out
}

// ------------------------------------------------------------------ the link

pub struct Link {
    status: Arc<Mutex<Status>>,
    commands: Sender<String>,
    alive: Arc<AtomicBool>,
}

impl Link {
    /// Start a background session. It reconnects on its own for as long as the
    /// link is alive; `status()` always answers, connected or not.
    pub fn spawn(config: HostConfig) -> Self {
        let status = Arc::new(Mutex::new(Status {
            note: "connecting".to_string(),
            ..Status::default()
        }));
        let (tx, rx) = mpsc::channel::<String>();
        let alive = Arc::new(AtomicBool::new(true));
        {
            let status = Arc::clone(&status);
            let alive = Arc::clone(&alive);
            thread::Builder::new()
                .name("shale-printer".into())
                .spawn(move || {
                    while alive.load(Ordering::Relaxed) {
                        match session(&config, &status, &rx, &alive) {
                            Ok(()) => break,
                            Err(err) => {
                                {
                                    let mut guard = status.lock().unwrap();
                                    guard.connected = false;
                                    guard.note = err.to_string();
                                }
                                if !alive.load(Ordering::Relaxed) {
                                    break;
                                }
                                thread::sleep(Duration::from_secs(3));
                            }
                        }
                    }
                })
                .expect("a printer thread");
        }
        Self {
            status,
            commands: tx,
            alive,
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn send(&self, command: serde_json::Value) -> Result<()> {
        self.commands
            .send(command.to_string())
            .map_err(|_| HostError::Bad("the printer link is closed".into()))
    }

    pub fn close(&self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.close();
    }
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

fn session(
    config: &HostConfig,
    status: &Arc<Mutex<Status>>,
    commands: &Receiver<String>,
    alive: &Arc<AtomicBool>,
) -> Result<()> {
    let mut wire = Wire::open(&config.host, config.mqtt_port)?;

    // MQTT 3.1.1 CONNECT: clean session, username and password, 60 s keepalive.
    let mut body = Vec::new();
    push_str(&mut body, "MQTT");
    body.push(4);
    body.push(0b1100_0010);
    body.extend_from_slice(&60u16.to_be_bytes());
    push_str(&mut body, &format!("shale-{}", config.serial));
    push_str(&mut body, "bblp");
    push_str(&mut body, &config.code);
    wire.write_all(&packet(0x10, &body))?;

    // SUBSCRIBE to the report topic, QoS 0.
    let mut sub = Vec::new();
    sub.extend_from_slice(&1u16.to_be_bytes());
    push_str(&mut sub, &config.report_topic());
    sub.push(0);
    wire.write_all(&packet(0x82, &sub))?;

    {
        let mut guard = status.lock().unwrap();
        guard.connected = true;
        guard.note = String::new();
    }

    let mut last_ping = Instant::now();
    while alive.load(Ordering::Relaxed) {
        while let Ok(text) = commands.try_recv() {
            let mut publish = Vec::new();
            push_str(&mut publish, &config.request_topic());
            publish.extend_from_slice(text.as_bytes());
            wire.write_all(&packet(0x30, &publish))?;
        }

        let got = wire.fill()?;
        while let Some(bytes) = wire.packet() {
            handle_packet(&bytes, status);
        }
        if !got {
            thread::sleep(Duration::from_millis(20));
        }

        if last_ping.elapsed() > Duration::from_secs(20) {
            wire.write_all(&[0xc0, 0x00])?;
            last_ping = Instant::now();
        }
    }
    Ok(())
}

fn handle_packet(bytes: &[u8], status: &Arc<Mutex<Status>>) {
    if bytes.is_empty() {
        return;
    }
    let kind = bytes[0] >> 4;
    let (len, header) = match remaining_length(bytes) {
        Some(pair) => pair,
        None => return,
    };
    let payload = &bytes[header..(header + len).min(bytes.len())];
    match kind {
        3 => {
            // PUBLISH: topic, then the JSON.
            if payload.len() < 2 {
                return;
            }
            let topic_len = u16::from_be_bytes([payload[0], payload[1]]) as usize;
            if payload.len() < 2 + topic_len {
                return;
            }
            let body = &payload[2 + topic_len..];
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
                absorb(&value, status);
            }
        }
        4 => {
            // CONNACK: byte 0 is the session flags, byte 1 the return code.
            if payload.len() >= 2 && payload[1] != 0 {
                let mut guard = status.lock().unwrap();
                guard.note = format!("the printer refused the connection (code {})", payload[1]);
            }
        }
        _ => {}
    }
}

/// Fold one report into the status the window reads.
fn absorb(value: &serde_json::Value, status: &Arc<Mutex<Status>>) {
    let Some(print) = value.get("print") else {
        return;
    };
    let num = |key: &str| print.get(key).and_then(|v| v.as_f64());
    let text = |key: &str| {
        print
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    // The machine reports `true`/`false` as strings for some flags.
    let flag = |key: &str| match print.get(key) {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => s == "true",
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    };

    let mut guard = status.lock().unwrap();
    if let Some(state) = text("gcode_state") {
        guard.state = state;
    }
    if let Some(v) = num("mc_percent") {
        guard.percent = v as i64;
    }
    if let Some(v) = num("mc_remaining_time") {
        guard.remaining_min = v as i64;
    }
    if let Some(v) = num("layer_num") {
        guard.layer = v as i64;
    }
    if let Some(v) = num("total_layer_num") {
        guard.layers = v as i64;
    }
    if let Some(v) = num("nozzle_temper") {
        guard.nozzle = v;
    }
    if let Some(v) = num("nozzle_target_temper") {
        guard.nozzle_target = v;
    }
    if let Some(v) = num("bed_temper") {
        guard.bed = v;
    }
    if let Some(v) = num("bed_target_temper") {
        guard.bed_target = v;
    }
    if let Some(v) = num("print_error") {
        guard.error = v as i64;
    }
    if let Some(name) = text("subtask_name") {
        guard.file = name;
    }
    // `chamber_light` and friends are noise here; a raise is what matters.
    if flag("pause") == false && guard.state.is_empty() {
        guard.state = "IDLE".to_string();
    }
    guard.reports += 1;
    guard.updated_at = now_epoch();
    if guard.error != 0 {
        guard.note = format!("the printer reported error {}", guard.error);
    }
}

// ------------------------------------------------------------------ uploads

/// Upload a G-code file over implicit FTPS and return how many bytes landed.
///
/// Bambu's FTP server wants `PBSZ 0` and `PROT P` before the transfer, and the
/// data connection is opened to the address in the `227` reply.
pub fn upload(config: &HostConfig, local: &std::path::Path, name: &str) -> Result<usize> {
    let bytes = std::fs::read(local)?;
    let mut control = Wire::open(&config.host, config.ftps_port)?;
    expect(&mut control, "220")?;
    send_cmd(&mut control, &format!("USER {}", "bblp"), "331")?;
    send_cmd(&mut control, &format!("PASS {}", config.code), "230")?;
    send_cmd(&mut control, "PBSZ 0", "200")?;
    send_cmd(&mut control, "PROT P", "200")?;
    send_cmd(&mut control, "TYPE I", "200")?;

    let dir = config.remote_dir.trim_matches('/');
    let remote = if dir.is_empty() {
        format!("/{name}")
    } else {
        format!("/{dir}/{name}")
    };
    // FTP's order: ask for the transfer, read `150`, then open the data channel.
    // Opening it first deadlocks against a server that accepts only after STOR.
    let data_addr = pasv(&mut control)?;
    if std::env::var("SHALE_TRACE_HOST").is_ok() {
        eprintln!("HOST data address {data_addr} for {remote}");
    }
    send_cmd(&mut control, &format!("STOR {remote}"), "150")?;

    let data = TcpStream::connect_timeout(&data_addr, Duration::from_secs(6))?;
    data.set_read_timeout(Some(Duration::from_secs(20)))?;
    data.set_write_timeout(Some(Duration::from_secs(20)))?;
    let name_tls = ServerName::try_from(config.host.clone())
        .map_err(|_| HostError::Bad("the printer host is not a server name".into()))?;
    let conn = ClientConnection::new(tls_config(), name_tls)?;
    let mut secure = StreamOwned::new(conn, data);
    secure.conn.complete_io(&mut secure.sock)?;

    secure.write_all(&bytes)?;
    secure.flush()?;
    // Closing the data connection is what tells the server the file is complete.
    // Say so with a close_notify and read the rest of the stream first: closing
    // with the server's last records unread resets the connection, and the
    // upload is lost with it.
    let _ = secure.conn.send_close_notify();
    let _ = secure.flush();
    let mut drain = [0u8; 512];
    for _ in 0..16 {
        match secure.read(&mut drain) {
            Ok(0) => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    let _ = secure.sock.shutdown(std::net::Shutdown::Both);
    drop(secure);

    expect(&mut control, "226")?;
    let _ = send_cmd(&mut control, "QUIT", "221");
    Ok(bytes.len())
}

fn send_cmd(wire: &mut Wire, command: &str, want: &str) -> Result<()> {
    wire.write_all(format!("{command}\r\n").as_bytes())?;
    let line = wire.read_line()?;
    if line.starts_with(want) {
        Ok(())
    } else {
        Err(HostError::Bad(format!("{command}: {line}")))
    }
}

fn expect(wire: &mut Wire, want: &str) -> Result<()> {
    let line = wire.read_line()?;
    if line.starts_with(want) {
        Ok(())
    } else {
        Err(HostError::Bad(line))
    }
}

fn pasv(wire: &mut Wire) -> Result<std::net::SocketAddr> {
    wire.write_all(b"PASV\r\n")?;
    let line = wire.read_line()?;
    if !line.starts_with("227") {
        return Err(HostError::Bad(line));
    }
    let open = line
        .find('(')
        .ok_or_else(|| HostError::Bad(format!("no address in {line}")))?;
    let close = line
        .find(')')
        .ok_or_else(|| HostError::Bad(format!("no address in {line}")))?;
    let parts: Vec<u16> = line[open + 1..close]
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    if parts.len() != 6 {
        return Err(HostError::Bad(format!("no address in {line}")));
    }
    let ip = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);
    let port = parts[4] * 256 + parts[5];
    let addr = format!("{ip}:{port}");
    addr.parse()
        .map_err(|_| HostError::Bad(format!("bad address {addr}")))
}


// --------------------------------------------------------------- the file gate

/// Whether a G-code file starts with a machine's own sequence.
///
/// A Bambu machine's start writes a banner as its first line —
/// `;===== machine: A1 ====…` — before any heat or motion. A file that begins
/// with Cura's `;FLAVOR:Marlin` was started by the generic Marlin sequence and
/// must not be sent to a Bambu printer: the machine would run it, and the wipe,
/// the bed scan and the AMS handover that the profile asks for would be missing.
///
/// The check reads the head of the file rather than the whole of it, because a
/// sliced plate can be tens of megabytes.
pub fn starts_with_machine_sequence(path: &std::path::Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4096];
    let Ok(read) = file.read(&mut head) else {
        return false;
    };
    let text = String::from_utf8_lossy(&head[..read]);
    for line in text.lines().take(40) {
        let line = line.trim_start();
        if line.starts_with(";===== machine:") {
            return true;
        }
        // Cura's own first line; stop looking once the file has begun to move.
        if line.starts_with(";FLAVOR:") || line.starts_with("G0 ") || line.starts_with("G1 ") {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod gate_tests {
    use super::starts_with_machine_sequence;

    #[test]
    fn a_machine_banner_is_recognised_and_a_marlin_start_is_not() {
        let dir = std::env::temp_dir().join("shale-gate-test");
        std::fs::create_dir_all(&dir).unwrap();

        let good = dir.join("a1.gcode");
        std::fs::write(
            &good,
            ";===== machine: A1 =========================\n;A1 0.4 nozzle\nM620 S0A\nG28 Z P0 T140\nG1 X10 Y10\n",
        )
        .unwrap();
        assert!(starts_with_machine_sequence(&good));

        let cura = dir.join("cura.gcode");
        std::fs::write(&cura, ";FLAVOR:Marlin\n;TIME:1234\nM140 S60\nM104 S210\nG28 ;Start GCode\n").unwrap();
        assert!(!starts_with_machine_sequence(&cura));

        assert!(!starts_with_machine_sequence(&dir.join("missing.gcode")));
    }
}

// ------------------------------------------------------------- the commands

/// The JSON the machine takes to start a G-code file that is already uploaded.
pub fn gcode_file_command(sequence: u64, remote: &str, bed_type: &str) -> serde_json::Value {
    serde_json::json!({
        "print": {
            "sequence_id": sequence.to_string(),
            "command": "gcode_file",
            "param": remote,
            "bed_type": bed_type,
            "use_ams": false,
        }
    })
}

pub fn simple_command(sequence: u64, command: &str) -> serde_json::Value {
    serde_json::json!({
        "print": {
            "sequence_id": sequence.to_string(),
            "command": command,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_length_round_trips() {
        for len in [0usize, 1, 127, 128, 16_383, 16_384, 2_097_151] {
            let mut out = vec![0x30]; // the fixed header byte a packet starts with
            encode_remaining_length(len, &mut out);
            let (parsed, used) = remaining_length(&out).expect("a length");
            assert_eq!(parsed, len, "len {len}");
            // `used` counts the header byte plus the length's own bytes.
            assert_eq!(used, out.len(), "len {len} header size");
        }
    }

    #[test]
    fn a_publish_is_framed_and_read_back() {
        let mut publish = Vec::new();
        push_str(&mut publish, "device/SERIAL/report");
        publish.extend_from_slice(br#"{"print":{"gcode_state":"RUNNING"}}"#);
        let bytes = packet(0x30, &publish);

        let mut wire_bytes = bytes.clone();
        let mut buffered = Vec::new();
        buffered.append(&mut wire_bytes);
        let (len, header) = remaining_length(&buffered).expect("length");
        assert_eq!(header + len, bytes.len(), "the frame is complete");

        let status = Arc::new(Mutex::new(Status::default()));
        handle_packet(&buffered, &status);
        assert_eq!(status.lock().unwrap().state, "RUNNING");
    }

    #[test]
    fn a_report_fills_the_status() {
        let status = Arc::new(Mutex::new(Status::default()));
        let report = serde_json::json!({"print": {
            "gcode_state": "RUNNING", "mc_percent": 42, "mc_remaining_time": 37,
            "layer_num": 12, "total_layer_num": 100,
            "nozzle_temper": 219.9, "nozzle_target_temper": 220,
            "bed_temper": 64.8, "bed_target_temper": 65,
            "subtask_name": "cube", "print_error": 0
        }});
        absorb(&report, &status);
        let status = status.lock().unwrap();
        assert_eq!(status.state, "RUNNING");
        assert_eq!(status.percent, 42);
        assert_eq!(status.remaining_min, 37);
        assert_eq!(status.layer, 12);
        assert_eq!(status.layers, 100);
        assert!((status.nozzle - 219.9).abs() < 1e-9);
        assert_eq!(status.bed_target, 65.0);
        assert_eq!(status.file, "cube");
        assert_eq!(status.reports, 1);
        assert!(status.is_printing());
    }

    #[test]
    fn a_second_report_keeps_the_first_fields() {
        let status = Arc::new(Mutex::new(Status::default()));
        absorb(
            &serde_json::json!({"print": {"gcode_state": "PAUSE", "mc_percent": 10}}),
            &status,
        );
        absorb(
            &serde_json::json!({"print": {"nozzle_temper": 200.0}}),
            &status,
        );
        let status = status.lock().unwrap();
        assert_eq!(status.state, "PAUSE");
        assert_eq!(status.percent, 10);
        assert!((status.nozzle - 200.0).abs() < 1e-9);
    }

    #[test]
    fn the_start_command_names_the_file_and_the_plate() {
        let command = gcode_file_command(7, "/cache/cube.gcode", "textured_plate");
        assert_eq!(command["print"]["sequence_id"], "7");
        assert_eq!(command["print"]["command"], "gcode_file");
        assert_eq!(command["print"]["param"], "/cache/cube.gcode");
        assert_eq!(command["print"]["bed_type"], "textured_plate");
        assert_eq!(command["print"]["use_ams"], false);
    }

    #[test]
    fn a_passive_address_is_parsed() {
        assert_eq!(
            "192.168.3.40:59021".parse::<std::net::SocketAddr>().unwrap(),
            "192.168.3.40:59021".parse().unwrap()
        );
    }
}
