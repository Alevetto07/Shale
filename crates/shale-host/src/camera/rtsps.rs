//! RTSPS on port 322.
//!
//! The P2S, the X1 series and the H2 series serve
//! `rtsps://<host>:322/streaming/live/1`. The user is `bblp` and the password
//! is the LAN access code. Shale asks for RTP interleaved on the same TLS
//! socket (`RTP/AVP/TCP;unicast;interleaved=0-1`), so a test does not have to
//! open a UDP port.
//!
//! The P2S picture is H.264. This client completes the RTSP handshake,
//! reassembles NAL units, and — when a packet is a JPEG, payload type 26 or
//! a payload that already starts with `FF D8` — returns that JPEG. It does
//! not decode H.264. A still from a P2S therefore needs a decoder this build
//! does not ship; the panel still shows that the stream is up.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::{connect_tls, read_exact, write_all, CameraFault, CameraTarget, Frame};

pub fn stream(
    target: &CameraTarget,
    stop: &AtomicBool,
    mut on_frame: impl FnMut(Frame) -> bool,
) -> Result<(), CameraFault> {
    let mut stream = connect_tls(&target.host, target.port, Duration::from_secs(6))?;
    let url = format!("rtsps://{}:{}/streaming/live/1", target.host, target.port);
    let auth = basic_auth(target.access_code());
    let mut cseq = 1u32;

    let options = request("OPTIONS", &url, cseq, &auth, None);
    cseq += 1;
    write_all(&mut stream, options.as_bytes())?;
    let options_reply = read_response(&mut stream)?;
    if options_reply.status == 401 {
        return Err(CameraFault::WrongAccessCode);
    }
    expect_ok(&options_reply)?;

    let describe = request(
        "DESCRIBE",
        &url,
        cseq,
        &auth,
        Some("Accept: application/sdp"),
    );
    cseq += 1;
    write_all(&mut stream, describe.as_bytes())?;
    let described = read_response(&mut stream)?;
    if described.status == 401 {
        return Err(CameraFault::WrongAccessCode);
    }
    expect_ok(&described)?;
    let sdp = String::from_utf8_lossy(&described.body);
    let control = control_url(&url, &sdp);
    let setup = request(
        "SETUP",
        &control,
        cseq,
        &auth,
        Some("Transport: RTP/AVP/TCP;unicast;interleaved=0-1"),
    );
    cseq += 1;
    write_all(&mut stream, setup.as_bytes())?;
    let setup_reply = read_response(&mut stream)?;
    expect_ok(&setup_reply)?;
    let session = header(&setup_reply, "session")
        .ok_or_else(|| CameraFault::Protocol("the camera's SETUP reply had no session".into()))?;
    let session_id = session.split(';').next().unwrap_or(session).trim();

    let play = format!(
        "PLAY {url} RTSP/1.0\r\nCSeq: {cseq}\r\nAuthorization: {auth}\r\nSession: {session_id}\r\nRange: npt=0.000-\r\n\r\n"
    );
    write_all(&mut stream, play.as_bytes())?;
    let playing = read_response(&mut stream)?;
    expect_ok(&playing)?;

    let mut assembly = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let packet = read_interleaved(&mut stream)?;
        if packet.channel != 0 {
            continue;
        }
        let Some(rtp) = parse_rtp(&packet.payload) else {
            continue;
        };
        if rtp.payload_type == 26 || rtp.payload.starts_with(&[0xff, 0xd8]) {
            let jpeg = jpeg_from_rtp(rtp.payload);
            if jpeg.starts_with(&[0xff, 0xd8]) && !on_frame(Frame::Jpeg(jpeg)) {
                break;
            }
            continue;
        }
        if let Some(unit) = depacketise_h264(&mut assembly, rtp.payload, rtp.marker) {
            if !on_frame(Frame::H264(unit)) {
                break;
            }
        }
    }
    Ok(())
}

fn basic_auth(access_code: &str) -> String {
    // The standard base64 alphabet, so the crate does not take a second
    // dependency for one header. The code itself never leaves this function
    // into an error string.
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let raw = format!("bblp:{access_code}");
    let bytes = raw.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | bytes[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        i += 3;
    }
    if i < bytes.len() {
        let left = bytes.len() - i;
        let b0 = bytes[i] as u32;
        let b1 = if left == 2 { bytes[i + 1] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8);
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
    format!("Basic {out}")
}

fn request(method: &str, url: &str, cseq: u32, auth: &str, extra: Option<&str>) -> String {
    let mut text = format!("{method} {url} RTSP/1.0\r\nCSeq: {cseq}\r\nAuthorization: {auth}\r\n");
    if let Some(extra) = extra {
        text.push_str(extra);
        text.push_str("\r\n");
    }
    text.push_str("\r\n");
    text
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn expect_ok(response: &Response) -> Result<(), CameraFault> {
    if response.status == 401 {
        return Err(CameraFault::WrongAccessCode);
    }
    if response.status == 404 || response.status == 454 || response.status == 503 {
        return Err(CameraFault::LanLiveViewOff);
    }
    if !(200..300).contains(&response.status) {
        return Err(CameraFault::Protocol(format!(
            "the camera answered RTSP {}",
            response.status
        )));
    }
    Ok(())
}

fn read_response(stream: &mut impl Read) -> Result<Response, CameraFault> {
    let mut collected = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        read_exact(stream, &mut byte).map_err(|err| match err {
            CameraFault::WrongAccessCode if collected.is_empty() => CameraFault::LanLiveViewOff,
            other => other,
        })?;
        collected.push(byte[0]);
        if collected.ends_with(b"\r\n\r\n") {
            break;
        }
        if collected.len() > 16_384 {
            return Err(CameraFault::Protocol(
                "the camera's RTSP reply did not end".into(),
            ));
        }
    }
    let text = String::from_utf8_lossy(&collected);
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|word| word.parse().ok())
        .ok_or_else(|| CameraFault::Protocol("the camera's RTSP status was not a number".into()))?;
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
        read_exact(stream, &mut body)?;
    }
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn control_url(base: &str, sdp: &str) -> String {
    let control = sdp.lines().rev().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("a=control:")
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "*")
    });
    match control {
        Some(value) if value.starts_with("rtsp") => value.to_string(),
        Some(value) => format!("{base}/{value}"),
        None => format!("{base}/trackID=1"),
    }
}

struct Interleaved {
    channel: u8,
    payload: Vec<u8>,
}

fn read_interleaved(stream: &mut impl Read) -> Result<Interleaved, CameraFault> {
    // After PLAY the printer may still have written the response we already
    // consumed. A `$` byte starts an interleaved RTP packet.
    let mut byte = [0u8; 1];
    loop {
        read_exact(stream, &mut byte)?;
        if byte[0] == b'$' {
            break;
        }
    }
    let mut head = [0u8; 3];
    read_exact(stream, &mut head)?;
    let channel = head[0];
    let length = u16::from_be_bytes([head[1], head[2]]) as usize;
    if length > 1_000_000 {
        return Err(CameraFault::Protocol(
            "the camera sent an RTP packet that is far too large".into(),
        ));
    }
    let mut payload = vec![0u8; length];
    read_exact(stream, &mut payload)?;
    Ok(Interleaved { channel, payload })
}

struct Rtp<'a> {
    marker: bool,
    payload_type: u8,
    payload: &'a [u8],
}

fn parse_rtp(packet: &[u8]) -> Option<Rtp<'_>> {
    if packet.len() < 12 || packet[0] >> 6 != 2 {
        return None;
    }
    let cc = (packet[0] & 0x0f) as usize;
    let extension = packet[0] & 0x10 != 0;
    let marker = packet[1] & 0x80 != 0;
    let payload_type = packet[1] & 0x7f;
    let mut offset = 12 + cc * 4;
    if extension {
        if packet.len() < offset + 4 {
            return None;
        }
        let words = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]) as usize;
        offset += 4 + words * 4;
    }
    if offset > packet.len() {
        return None;
    }
    Some(Rtp {
        marker,
        payload_type,
        payload: &packet[offset..],
    })
}

fn jpeg_from_rtp(payload: &[u8]) -> Vec<u8> {
    if payload.starts_with(&[0xff, 0xd8]) {
        return payload.to_vec();
    }
    // RFC 2435 puts an 8-byte header in front of the scan. If a JPEG marker
    // sits just after it, that is the picture.
    if payload.len() > 8 && payload[8..].starts_with(&[0xff, 0xd8]) {
        return payload[8..].to_vec();
    }
    payload.to_vec()
}

/// Reassemble one H.264 access unit from an RTP payload.
///
/// Single NAL units (types 1–23) and FU-A fragments (type 28) are the two
/// forms the P2S stream uses. The returned buffer is Annex-B.
pub fn depacketise_h264(assembly: &mut Vec<u8>, payload: &[u8], marker: bool) -> Option<Vec<u8>> {
    if payload.is_empty() {
        return None;
    }
    let nal_type = payload[0] & 0x1f;
    if nal_type == 28 {
        if payload.len() < 2 {
            return None;
        }
        let fu = payload[1];
        let start = fu & 0x80 != 0;
        let end = fu & 0x40 != 0;
        let reconstructed = (payload[0] & 0xe0) | (fu & 0x1f);
        if start {
            assembly.clear();
            assembly.extend_from_slice(&[0, 0, 0, 1, reconstructed]);
        }
        assembly.extend_from_slice(&payload[2..]);
        if end || marker {
            return Some(std::mem::take(assembly));
        }
        return None;
    }
    if (1..24).contains(&nal_type) {
        let mut unit = vec![0, 0, 0, 1];
        unit.extend_from_slice(payload);
        return Some(unit);
    }
    None
}

/// An RTP packet, version 2, no extension. Tests and the mock server share it.
pub fn encode_rtp(payload_type: u8, marker: bool, payload: &[u8]) -> Vec<u8> {
    let mut packet = vec![0u8; 12 + payload.len()];
    packet[0] = 0x80;
    packet[1] = payload_type & 0x7f;
    if marker {
        packet[1] |= 0x80;
    }
    packet[12..].copy_from_slice(payload);
    packet
}

/// `$` / channel / length, the interleaved framing RTSP uses on one socket.
pub fn encode_interleaved_rtp(channel: u8, rtp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + rtp.len());
    out.push(b'$');
    out.push(channel);
    out.extend_from_slice(&(rtp.len() as u16).to_be_bytes());
    out.extend_from_slice(rtp);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fragmented_nal_is_reassembled_in_order() {
        let mut assembly = Vec::new();
        let first = depacketise_h264(&mut assembly, &[0x7c, 0x81, 0xaa, 0xbb], false);
        assert!(first.is_none());
        let unit = depacketise_h264(&mut assembly, &[0x7c, 0x41, 0xcc], true).expect("end");
        assert_eq!(unit, vec![0, 0, 0, 1, 0x61, 0xaa, 0xbb, 0xcc]);
    }

    #[test]
    fn basic_auth_does_not_appear_in_the_fault_text() {
        let header = basic_auth("12345678");
        assert!(header.starts_with("Basic "));
        let fault = CameraFault::WrongAccessCode.message();
        assert!(!fault.contains("12345678"));
        assert!(!header.contains("12345678"));
    }

    #[test]
    fn interleaved_rtp_round_trips_a_jpeg_marker() {
        let jpeg = [0xff, 0xd8, 0xff, 0xd9];
        let rtp = encode_rtp(26, true, &jpeg);
        let framed = encode_interleaved_rtp(0, &rtp);
        assert_eq!(framed[0], b'$');
        assert_eq!(
            u16::from_be_bytes([framed[2], framed[3]]) as usize,
            rtp.len()
        );
        let parsed = parse_rtp(&framed[4..]).expect("rtp");
        assert!(parsed.marker);
        assert_eq!(parsed.payload_type, 26);
        assert_eq!(parsed.payload, &jpeg);
    }
}
