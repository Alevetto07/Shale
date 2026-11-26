//! The chamber camera, against a printer that exists only on loopback.
//!
//! The bytes are the ones OpenBambuAPI records: an 80-byte authentication
//! packet and a 16-byte JPEG header on port 6000, and an RTSP handshake plus
//! interleaved RTP on port 322. Nothing here is a real printer, and nothing
//! is told to move.

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use image::{DynamicImage, Rgb, RgbImage};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use shale_host::camera::{
    auth_packet, path_for_model, snapshot_jpeg, CameraFault, CameraPath, CameraSession, CameraTarget,
};
use support::fake_printer::{CERT_A, KEY_A};

fn jpeg(grey: u8) -> Vec<u8> {
    let image = RgbImage::from_pixel(32, 24, Rgb([grey, grey, grey]));
    let mut bytes = Vec::new();
    DynamicImage::ImageRgb8(image)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .expect("a jpeg");
    assert!(bytes.starts_with(&[0xff, 0xd8]));
    bytes
}

fn server_config() -> Arc<ServerConfig> {
    let certs = vec![CertificateDer::from(CERT_A.to_vec())];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(KEY_A.to_vec()));
    Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("TLS")
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .expect("fixture certificate"),
    )
}

enum JpegMode {
    Frames(Vec<Vec<u8>>),
    LiveViewOff,
    CameraOff,
}

struct JpegServer {
    port: u16,
    seen: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl JpegServer {
    fn start(code: &'static str, mode: JpegMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = server_config();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let _ = serve_jpeg(stream, config, code, mode, record);
        });
        Self { port, seen }
    }
}

fn serve_jpeg(
    stream: TcpStream,
    config: Arc<ServerConfig>,
    code: &str,
    mode: JpegMode,
    seen: Arc<Mutex<Vec<Vec<u8>>>>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let conn = ServerConnection::new(config).unwrap();
    let mut tls = StreamOwned::new(conn, stream);
    let mut auth = [0u8; 80];
    tls.read_exact(&mut auth)?;
    seen.lock().unwrap().push(auth.to_vec());
    let password = String::from_utf8_lossy(&auth[48..64]).trim_matches('\0').to_string();
    if password != code {
        return Ok(());
    }
    match mode {
        JpegMode::LiveViewOff => {
            let mut header = [0u8; 16];
            header[0..4].copy_from_slice(&8u32.to_le_bytes());
            header[4..8].copy_from_slice(&0x0003_013fu32.to_le_bytes());
            tls.write_all(&header)?;
            tls.write_all(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0])?;
        }
        JpegMode::CameraOff => {
            tls.write_all(&[0u8; 16])?;
        }
        JpegMode::Frames(frames) => {
            for frame in frames {
                let mut header = [0u8; 16];
                header[0..4].copy_from_slice(&(frame.len() as u32).to_le_bytes());
                header[8..12].copy_from_slice(&1u32.to_le_bytes());
                tls.write_all(&header)?;
                tls.write_all(&frame)?;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    Ok(())
}

fn target(port: u16, code: &str, path: CameraPath) -> CameraTarget {
    CameraTarget::new("127.0.0.1", "bambu_lab_a1", code)
        .with_port(port)
        .with_path(path)
}

#[test]
fn a_jpeg_camera_returns_the_recorded_frame_and_refuses_a_wrong_code() {
    let frame = jpeg(40);
    let server = JpegServer::start("12345678", JpegMode::Frames(vec![frame.clone()]));
    let got = snapshot_jpeg(&target(server.port, "12345678", CameraPath::JpegTls), Duration::from_secs(3))
        .expect("frame");
    assert_eq!(got, frame);
    let packet = auth_packet("bblp", "12345678");
    assert_eq!(server.seen.lock().unwrap()[0], packet);

    let wrong = JpegServer::start("12345678", JpegMode::Frames(vec![frame]));
    let err = snapshot_jpeg(&target(wrong.port, "00000000", CameraPath::JpegTls), Duration::from_secs(3))
        .expect_err("wrong code");
    assert_eq!(err, CameraFault::WrongAccessCode);
    assert!(!err.to_string().contains("00000000"));
    assert!(!err.to_string().contains("12345678"));
}

#[test]
fn live_view_off_and_a_disabled_camera_are_named() {
    let live = JpegServer::start("12345678", JpegMode::LiveViewOff);
    let err = snapshot_jpeg(&target(live.port, "12345678", CameraPath::JpegTls), Duration::from_secs(3))
        .unwrap_err();
    assert_eq!(err, CameraFault::LanLiveViewOff);
    assert!(err.to_string().contains("LAN-only live view"));

    let off = JpegServer::start("12345678", JpegMode::CameraOff);
    let err = snapshot_jpeg(&target(off.port, "12345678", CameraPath::JpegTls), Duration::from_secs(3)).unwrap_err();
    assert_eq!(err, CameraFault::CameraDisabled);
}

#[test]
fn a_session_counts_frames_from_the_mock_and_stops() {
    let frame = jpeg(90);
    let server = JpegServer::start("12345678", JpegMode::Frames(vec![frame.clone(), frame]));
    let session = CameraSession::start(target(server.port, "12345678", CameraPath::JpegTls));
    let mut seen = 0;
    for _ in 0..40 {
        let status = session.status();
        seen = status.frames;
        if seen >= 1 && status.has_jpeg {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    session.stop();
    assert!(seen >= 1, "frames {seen}");
    assert!(session.latest_jpeg().unwrap().starts_with(&[0xff, 0xd8]));
}

struct RtspServer {
    port: u16,
}

impl RtspServer {
    fn start(reply: RtspReply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = server_config();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let _ = serve_rtsp(stream, config, reply);
        });
        Self { port }
    }
}

enum RtspReply {
    Jpeg(Vec<u8>),
    H264,
    AnnexB(Vec<u8>),
    Unauthorised,
    Closed,
}

fn serve_rtsp(stream: TcpStream, config: Arc<ServerConfig>, reply: RtspReply) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let conn = ServerConnection::new(config).unwrap();
    let mut tls = StreamOwned::new(conn, stream);
    if matches!(reply, RtspReply::Closed) {
        return Ok(());
    }
    for _ in 0..4 {
        let request = read_headers(&mut tls)?;
        if matches!(reply, RtspReply::Unauthorised) {
            tls.write_all(b"RTSP/1.0 401 Unauthorized\r\nCSeq: 1\r\n\r\n")?;
            return Ok(());
        }
        let response = if request.starts_with("OPTIONS") {
            "RTSP/1.0 200 OK\r\nCSeq: 1\r\nPublic: OPTIONS, DESCRIBE, SETUP, PLAY\r\n\r\n"
        } else if request.starts_with("DESCRIBE") {
            "RTSP/1.0 200 OK\r\nCSeq: 2\r\nContent-Type: application/sdp\r\nContent-Length: 88\r\n\r\n\
v=0\r\nm=video 0 RTP/AVP 26\r\na=rtpmap:26 JPEG/90000\r\na=control:trackID=1\r\n"
        } else if request.starts_with("SETUP") {
            "RTSP/1.0 200 OK\r\nCSeq: 3\r\nSession: 7\r\nTransport: RTP/AVP/TCP;unicast;interleaved=0-1\r\n\r\n"
        } else {
            "RTSP/1.0 200 OK\r\nCSeq: 4\r\nSession: 7\r\n\r\n"
        };
        // The DESCRIBE body length has to match. Count it rather than trust a
        // hand-written number if the SDP above is edited.
        if request.starts_with("DESCRIBE") {
            let sdp = "v=0\r\nm=video 0 RTP/AVP 26\r\na=rtpmap:26 JPEG/90000\r\na=control:trackID=1\r\n";
            let response = format!(
                "RTSP/1.0 200 OK\r\nCSeq: 2\r\nContent-Type: application/sdp\r\nContent-Length: {}\r\n\r\n{sdp}",
                sdp.len()
            );
            tls.write_all(response.as_bytes())?;
        } else {
            tls.write_all(response.as_bytes())?;
        }
        if request.starts_with("PLAY") {
            break;
        }
    }
    match reply {
        RtspReply::Jpeg(frame) => {
            let rtp = shale_host::camera::encode_rtp(26, true, &frame);
            let packet = shale_host::camera::encode_interleaved_rtp(0, &rtp);
            tls.write_all(&packet)?;
        }
        RtspReply::H264 => {
            let start = shale_host::camera::encode_rtp(96, false, &[0x7c, 0x81, 0x11, 0x22]);
            let end = shale_host::camera::encode_rtp(96, true, &[0x7c, 0x41, 0x33]);
            tls.write_all(&shale_host::camera::encode_interleaved_rtp(0, &start))?;
            tls.write_all(&shale_host::camera::encode_interleaved_rtp(0, &end))?;
        }
        RtspReply::AnnexB(bytes) => {
            for payload in shale_host::camera::nal_payloads(&bytes) {
                let kind = payload.first().copied().unwrap_or(0) & 0x1f;
                let marker = kind != 7 && kind != 8;
                let rtp = shale_host::camera::encode_rtp(96, marker, payload);
                tls.write_all(&shale_host::camera::encode_interleaved_rtp(0, &rtp))?;
            }
        }
        RtspReply::Unauthorised | RtspReply::Closed => {}
    }
    thread::sleep(Duration::from_millis(150));
    Ok(())
}

fn read_headers(stream: &mut impl Read) -> std::io::Result<String> {
    let mut collected = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        collected.push(byte[0]);
        if collected.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&collected).into_owned())
}

#[test]
fn an_rtsps_camera_returns_a_jpeg_packet_and_names_a_refused_code() {
    assert_eq!(path_for_model("bambu_lab_p2s"), CameraPath::Rtsps);
    let frame = jpeg(120);
    let server = RtspServer::start(RtspReply::Jpeg(frame.clone()));
    let got = snapshot_jpeg(
        &target(server.port, "12345678", CameraPath::Rtsps),
        Duration::from_secs(3),
    )
    .expect("jpeg over rtp");
    assert_eq!(got, frame);

    let denied = RtspServer::start(RtspReply::Unauthorised);
    let err = snapshot_jpeg(
        &target(denied.port, "12345678", CameraPath::Rtsps),
        Duration::from_secs(3),
    )
    .unwrap_err();
    assert_eq!(err, CameraFault::WrongAccessCode);

    let closed = RtspServer::start(RtspReply::Closed);
    let err = snapshot_jpeg(
        &target(closed.port, "12345678", CameraPath::Rtsps),
        Duration::from_secs(3),
    )
    .unwrap_err();
    assert_eq!(err, CameraFault::LanLiveViewOff);
}

#[test]
fn an_h264_stream_is_accepted_and_does_not_pretend_to_be_a_jpeg() {
    let server = RtspServer::start(RtspReply::H264);
    let err = snapshot_jpeg(
        &target(server.port, "12345678", CameraPath::Rtsps),
        Duration::from_secs(3),
    )
    .unwrap_err();
    assert!(err.to_string().contains("H.264"), "{err}");
    assert!(!err.to_string().contains("12345678"));
}

#[test]
fn a_p2s_h264_fixture_decodes_to_the_same_jpeg_pipeline() {
    let fixture = include_bytes!("fixtures/red-32.h264").to_vec();
    let server = RtspServer::start(RtspReply::AnnexB(fixture));
    // A release build has no decoder until Cisco's binary is downloaded, and
    // CI does not download it. The `openh264-source` feature is what decodes
    // the fixture here. Either way the access code stays out of the answer.
    match snapshot_jpeg(
        &target(server.port, "12345678", CameraPath::Rtsps),
        Duration::from_secs(5),
    ) {
        Ok(jpeg) => {
            assert!(jpeg.starts_with(&[0xff, 0xd8]), "the overlay path stores a JPEG");
            let image = image::ImageReader::new(std::io::Cursor::new(jpeg))
                .with_guessed_format()
                .expect("format")
                .decode()
                .expect("jpeg")
                .to_rgb8();
            assert_eq!(image.dimensions(), (32, 32));
            let pixel = image.get_pixel(16, 16);
            assert!(pixel[0] > 180 && pixel[1] < 80 && pixel[2] < 80, "{pixel:?}");
        }
        Err(err) => {
            let text = err.to_string();
            assert!(text.contains("H.264"), "{text}");
            assert!(text.contains("Cisco") || text.contains("OpenH264"), "{text}");
            assert!(!text.contains("12345678"), "{text}");
        }
    }
}
