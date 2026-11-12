// The protocol tables this double speaks are maps of tuples — a topic to a set
// of commands, a certificate to a fingerprint — and naming each shape would add
// types that say nothing a reader cannot see in place.
#![allow(clippy::type_complexity)]

//! A Bambu printer that exists only on 127.0.0.1.
//!
//! The host code was written against a machine on the other end of a cable, and
//! until now every claim about it rested on that machine being reachable. This
//! is the other end: MQTT over TLS on one loopback port, implicit FTPS on
//! another, both bound to ephemeral ports so a test needs no administrator
//! rights and cannot collide with anything.
//!
//! It speaks the part of the protocol Shale uses, and no more:
//!
//! * MQTT 3.1.1 — CONNECT/CONNACK, SUBSCRIBE/SUBACK, PUBLISH, PINGREQ/PINGRESP.
//!   Commands the host publishes to `device/<serial>/request` are collected;
//!   reports the test pushes are delivered to subscribers on
//!   `device/<serial>/report`.
//! * Implicit FTPS — a TLS control channel that accepts USER/PASS/PBSZ/PROT/
//!   TYPE, hands out a passive port, and reads a `STOR` payload over a second
//!   TLS connection.
//!
//! What it does not do is pretend to be a printer: it does not move, heat, or
//! interpret G-code. It records what it was given, which is exactly what a
//! wiring test needs to see.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

pub const CERT_A: &[u8] = include_bytes!("../fixtures/fake-tls/cert-a.der");
pub const KEY_A: &[u8] = include_bytes!("../fixtures/fake-tls/key-a.der");
// The second pair is what the pin test puts in front of a client that has
// already accepted the first.
pub const CERT_B: &[u8] = include_bytes!("../fixtures/fake-tls/cert-b.der");
pub const KEY_B: &[u8] = include_bytes!("../fixtures/fake-tls/key-b.der");

/// SHA-256 of the certificate each fixture carries, lower-case hex. Written
/// down here so a test can name the pin without hashing anything.
pub const FINGERPRINT_A: &str = "61acc0aeb5255c5d0548b5c3afefd8ec58c5a512a9672de9f4495fe1b3a26f11";
pub const FINGERPRINT_B: &str = "633d1a33fbe0a3cbe323f936f4e143dc31591aa8c2ebe5d86fe5ed970b810ebd";

pub struct FakePrinter {
    pub mqtt_port: u16,
    pub ftps_port: u16,
    /// Everything the host published on the request topic, in order.
    pub commands: Arc<Mutex<Vec<serde_json::Value>>>,
    /// Every file that arrived over FTPS, by remote name.
    pub uploads: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    /// Reports queued for the live subscriber.
    outboxes: Arc<Mutex<Vec<Arc<Mutex<Vec<u8>>>>>>,
    stop: Arc<AtomicBool>,
}

impl FakePrinter {
    /// A machine on the ports a test wants: 0 asks the system for a free one,
    /// which is what a test does so it never needs a privileged port or a free
    /// one that another run of itself is holding.
    pub fn start_on(mqtt_port: u16, ftps_port: u16, cert: &[u8], key: &[u8]) -> Self {
        let config = Arc::new(server_config(cert, key));
        let commands = Arc::new(Mutex::new(Vec::new()));
        let uploads = Arc::new(Mutex::new(Vec::new()));
        let outboxes: Arc<Mutex<Vec<Arc<Mutex<Vec<u8>>>>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let mqtt = TcpListener::bind(("127.0.0.1", mqtt_port)).expect("a loopback MQTT port");
        let ftps = TcpListener::bind(("127.0.0.1", ftps_port)).expect("a loopback FTPS port");
        let mqtt_port = mqtt.local_addr().unwrap().port();
        let ftps_port = ftps.local_addr().unwrap().port();

        {
            let config = Arc::clone(&config);
            let commands = Arc::clone(&commands);
            let outboxes = Arc::clone(&outboxes);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                for stream in mqtt.incoming() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(stream) = stream else { break };
                    let config = Arc::clone(&config);
                    let commands = Arc::clone(&commands);
                    let outboxes = Arc::clone(&outboxes);
                    let stop = Arc::clone(&stop);
                    thread::spawn(move || {
                        let _ = serve_mqtt(stream, config, commands, outboxes, stop);
                    });
                }
            });
        }
        {
            let uploads = Arc::clone(&uploads);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                for stream in ftps.incoming() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(stream) = stream else { break };
                    let config = Arc::clone(&config);
                    let uploads = Arc::clone(&uploads);
                    let stop = Arc::clone(&stop);
                    thread::spawn(move || {
                        let _ = serve_ftps(stream, config, uploads, stop);
                    });
                }
            });
        }

        Self {
            mqtt_port,
            ftps_port,
            commands,
            uploads,
            outboxes,
            stop,
        }
    }

    /// The ephemeral-port machine every wiring test uses.
    pub fn start(cert: &[u8], key: &[u8]) -> Self {
        Self::start_on(0, 0, cert, key)
    }

    /// The configuration the host would use to reach this machine.
    pub fn config(&self) -> shale_host::HostConfig {
        shale_host::HostConfig {
            host: "127.0.0.1".to_string(),
            serial: "FAKE01A000000001".to_string(),
            code: "fake-access-code".to_string(),
            bed_type: "textured_plate".to_string(),
            remote_dir: "cache".to_string(),
            mqtt_port: self.mqtt_port,
            ftps_port: self.ftps_port,
        }
    }

    /// Push one status report to every subscriber, the way a machine does.
    pub fn say(&self, report: serde_json::Value) {
        let frame = publish_frame(
            "device/FAKE01A000000001/report",
            report.to_string().as_bytes(),
        );
        for box_ in self.outboxes.lock().unwrap().iter() {
            box_.lock().unwrap().extend_from_slice(&frame);
        }
    }

    pub fn commands(&self) -> Vec<serde_json::Value> {
        self.commands.lock().unwrap().clone()
    }

    pub fn uploads(&self) -> Vec<(String, Vec<u8>)> {
        self.uploads.lock().unwrap().clone()
    }

    /// Wait until `check` is true or `limit` passes, without sleeping the whole
    /// time: the host polls its socket every 20 ms, and a test that sleeps for a
    /// second per assertion spends its life asleep.
    pub fn wait_for<T>(&self, limit: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if let Some(value) = check() {
                return Some(value);
            }
            thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for FakePrinter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn server_config(cert: &[u8], key: &[u8]) -> ServerConfig {
    let certs = vec![CertificateDer::from(cert.to_vec())];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.to_vec()));
    ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("TLS 1.2 and 1.3")
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("the fixture certificate and key belong together")
}

fn would_block(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

// ---------------------------------------------------------------------- MQTT

fn serve_mqtt(
    stream: TcpStream,
    config: Arc<ServerConfig>,
    commands: Arc<Mutex<Vec<serde_json::Value>>>,
    outboxes: Arc<Mutex<Vec<Arc<Mutex<Vec<u8>>>>>>,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_millis(50)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let conn = ServerConnection::new(config).expect("a server connection");
    let mut tls = StreamOwned::new(conn, stream);
    let outbox: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        match tls.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => pending.extend_from_slice(&buf[..n]),
            Err(err) if would_block(&err) => {}
            Err(_) => break,
        }

        while let Some((first, body)) = take_packet(&mut pending) {
            match first >> 4 {
                1 => tls.write_all(&[0x20, 0x02, 0x00, 0x00])?, // CONNACK, accepted
                8 => {
                    // SUBACK, echoing the packet id the client chose.
                    let id = u16::from_be_bytes([body[0], body[1]]);
                    tls.write_all(&[0x90, 0x03, (id >> 8) as u8, id as u8, 0x00])?;
                    outboxes.lock().unwrap().push(Arc::clone(&outbox));
                }
                3 => {
                    // PUBLISH: topic length, topic, then the JSON.
                    if body.len() >= 2 {
                        let len = u16::from_be_bytes([body[0], body[1]]) as usize;
                        let start = 2 + len;
                        if body.len() > start {
                            if let Ok(value) =
                                serde_json::from_slice::<serde_json::Value>(&body[start..])
                            {
                                commands.lock().unwrap().push(value);
                            }
                        }
                    }
                }
                12 => tls.write_all(&[0xd0, 0x00])?, // PINGRESP
                14 => return Ok(()),                 // DISCONNECT
                _ => {}
            }
        }

        let queued = {
            let mut box_ = outbox.lock().unwrap();
            std::mem::take(&mut *box_)
        };
        if !queued.is_empty() {
            tls.write_all(&queued)?;
            tls.flush()?;
        }
    }
    Ok(())
}

fn publish_frame(topic: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(2 + topic.len() + payload.len());
    body.extend_from_slice(&(topic.len() as u16).to_be_bytes());
    body.extend_from_slice(topic.as_bytes());
    body.extend_from_slice(payload);
    let mut out = vec![0x30];
    encode_remaining_length(body.len(), &mut out);
    out.extend_from_slice(&body);
    out
}

/// Split one complete MQTT packet off the front of `bytes`, if it is all there.
fn take_packet(bytes: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    if bytes.len() < 2 {
        return None;
    }
    let mut value = 0usize;
    let mut shift = 0;
    let mut header = None;
    for (index, byte) in bytes.iter().enumerate().skip(1) {
        value |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            header = Some(index + 1);
            break;
        }
        shift += 7;
    }
    let header = header?;
    if bytes.len() < header + value {
        return None;
    }
    let first = bytes[0];
    let body = bytes[header..header + value].to_vec();
    bytes.drain(..header + value);
    Some((first, body))
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

// ---------------------------------------------------------------------- FTPS

fn serve_ftps(
    stream: TcpStream,
    config: Arc<ServerConfig>,
    uploads: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_millis(200)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    // The data channel is a second TLS session with the same certificate.
    let data_config = Arc::clone(&config);
    let conn = ServerConnection::new(config).expect("a server connection");
    let mut tls = StreamOwned::new(conn, stream);
    tls.write_all(b"220 fake bamboo ftpd\r\n")?;
    tls.flush()?;

    let mut line = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 1024];
    let mut passive: Option<TcpListener> = None;

    while !stop.load(Ordering::Relaxed) {
        match tls.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => pending.extend_from_slice(&buf[..n]),
            Err(err) if would_block(&err) => {
                // A control channel that has gone quiet is still open; the host
                // sends one command at a time and waits for the reply.
                let _ = tls.flush();
                continue;
            }
            Err(_) => break,
        }
        while let Some(at) = pending.windows(2).position(|pair| pair == b"\r\n") {
            line.extend_from_slice(&pending[..at]);
            pending.drain(..at + 2);
            let text = String::from_utf8_lossy(&line).trim().to_string();
            line.clear();
            let upper = text.to_ascii_uppercase();
            if upper.starts_with("USER") {
                tls.write_all(b"331 password please\r\n")?;
            } else if upper.starts_with("PASS") {
                tls.write_all(b"230 logged in\r\n")?;
            } else if upper.starts_with("PBSZ")
                || upper.starts_with("PROT")
                || upper.starts_with("TYPE")
                || upper.starts_with("OPTS")
            {
                tls.write_all(b"200 ok\r\n")?;
            } else if upper.starts_with("PASV") {
                let listener = TcpListener::bind("127.0.0.1:0")?;
                let port = listener.local_addr()?.port();
                let reply = format!(
                    "227 Entering Passive Mode (127,0,0,1,{},{}).\r\n",
                    port / 256,
                    port % 256
                );
                tls.write_all(reply.as_bytes())?;
                passive = Some(listener);
            } else if upper.starts_with("STOR") {
                let name = text
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/unnamed")
                    .to_string();
                tls.write_all(b"150 send the file\r\n")?;
                tls.flush()?;
                let Some(listener) = passive.take() else {
                    tls.write_all(b"425 no passive channel\r\n")?;
                    continue;
                };
                listener.set_nonblocking(false)?;
                let (data, _) = listener.accept()?;
                data.set_read_timeout(Some(Duration::from_secs(10)))?;
                let dconn =
                    ServerConnection::new(Arc::clone(&data_config)).expect("a data connection");
                let mut dstream = StreamOwned::new(dconn, data);
                let mut file = Vec::new();
                let mut chunk = [0u8; 16384];
                loop {
                    match dstream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => file.extend_from_slice(&chunk[..n]),
                        Err(err) if would_block(&err) => continue,
                        Err(_) => break,
                    }
                }
                // Push the last records back so the client's close_notify is
                // answered, which is what makes it read `226` instead of a reset.
                dstream.conn.send_close_notify();
                let _ = dstream.flush();
                uploads.lock().unwrap().push((name, file));
                tls.write_all(b"226 file received\r\n")?;
            } else if upper.starts_with("QUIT") {
                tls.write_all(b"221 bye\r\n")?;
                tls.flush()?;
                return Ok(());
            } else if upper.starts_with("NOOP") {
                tls.write_all(b"200 ok\r\n")?;
            } else {
                tls.write_all(b"502 not implemented here\r\n")?;
            }
            tls.flush()?;
        }
    }
    Ok(())
}
