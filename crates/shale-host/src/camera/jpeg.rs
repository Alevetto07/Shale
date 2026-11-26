//! JPEG over TLS, port 6000.
//!
//! The packet layout is the one OpenBambuAPI writes down for the A1 and the
//! P1 series. Integers are little-endian.
//!
//! Authentication, 80 bytes:
//!
//! | offset | value |
//! | --- | --- |
//! | 0 | payload size `0x40` |
//! | 4 | type `0x3000` |
//! | 8 | flags `0` |
//! | 12 | `0` |
//! | 16 | username, ASCII, NUL-padded to 32 bytes (`bblp`) |
//! | 48 | access code, ASCII, NUL-padded to 32 bytes |
//!
//! Each frame is a 16-byte header (`payload size`, `itrack`, `flags`, `0`)
//! and then that many JPEG bytes, `FF D8` … `FF D9`.
//!
//! A 24-byte reject whose second word is `0x0003013f` is the LAN-live-view
//! refusal recorded against this port: the printer is there, and the live
//! view switch is not.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::{connect_tls, read_exact, write_all, CameraFault, CameraTarget, Frame};

pub struct JpegHeader {
    pub payload_size: u32,
    pub itrack: u32,
    pub flags: u32,
}

/// The 80-byte authentication packet. The access code is copied in and the
/// rest of its field stays zero.
pub fn auth_packet(user: &str, access_code: &str) -> [u8; 80] {
    let mut packet = [0u8; 80];
    packet[0..4].copy_from_slice(&0x40u32.to_le_bytes());
    packet[4..8].copy_from_slice(&0x3000u32.to_le_bytes());
    write_field(&mut packet[16..48], user);
    write_field(&mut packet[48..80], access_code);
    packet
}

fn write_field(slot: &mut [u8], text: &str) {
    let bytes = text.as_bytes();
    let n = bytes.len().min(slot.len());
    slot[..n].copy_from_slice(&bytes[..n]);
}

pub fn read_header(bytes: &[u8; 16]) -> JpegHeader {
    let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    JpegHeader {
        payload_size: word(0),
        itrack: word(4),
        flags: word(8),
    }
}

/// `0x0003013f` is the status word of the LAN-live-view reject.
const LAN_REJECT: u32 = 0x0003_013f;
const MAX_JPEG: u32 = 8_000_000;

pub fn interpret_header(header: &JpegHeader) -> Result<u32, CameraFault> {
    if header.payload_size == 8 && header.itrack == LAN_REJECT {
        return Err(CameraFault::LanLiveViewOff);
    }
    if header.payload_size == 0 {
        return Err(CameraFault::CameraDisabled);
    }
    if header.flags == 2 && header.payload_size < 64 {
        return Err(CameraFault::CameraDisabled);
    }
    if !(128..=MAX_JPEG).contains(&header.payload_size) {
        return Err(CameraFault::Protocol(format!(
            "the camera sent a frame header Shale does not understand ({} bytes)",
            header.payload_size
        )));
    }
    Ok(header.payload_size)
}

pub fn one_frame(target: &CameraTarget, timeout: Duration) -> Result<Vec<u8>, CameraFault> {
    let mut stream = connect_tls(&target.host, target.port, timeout)?;
    write_all(&mut stream, &auth_packet("bblp", target.access_code()))?;
    read_frame(&mut stream)
}

pub fn stream(
    target: &CameraTarget,
    stop: &AtomicBool,
    mut on_frame: impl FnMut(Frame) -> bool,
) -> Result<(), CameraFault> {
    let mut stream = connect_tls(&target.host, target.port, Duration::from_secs(6))?;
    write_all(&mut stream, &auth_packet("bblp", target.access_code()))?;
    while !stop.load(Ordering::Relaxed) {
        let jpeg = read_frame(&mut stream)?;
        if !on_frame(Frame::Jpeg(jpeg)) {
            break;
        }
    }
    Ok(())
}

fn read_frame(stream: &mut impl std::io::Read) -> Result<Vec<u8>, CameraFault> {
    let mut header_bytes = [0u8; 16];
    read_exact(stream, &mut header_bytes)?;
    let header = read_header(&header_bytes);
    let size = interpret_header(&header)? as usize;
    let mut body = vec![0u8; size];
    read_exact(stream, &mut body).map_err(|err| match err {
        CameraFault::WrongAccessCode => CameraFault::Protocol(
            "the camera closed the connection in the middle of a frame".into(),
        ),
        other => other,
    })?;
    if body.first() != Some(&0xff) || body.get(1) != Some(&0xd8) {
        // A short body that is not a JPEG, after a header we already accepted,
        // is the camera saying it has nothing to send.
        if size < 256 {
            return Err(CameraFault::CameraDisabled);
        }
        return Err(CameraFault::Protocol(
            "the camera's frame was not a JPEG".into(),
        ));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_auth_packet_is_eighty_bytes_with_the_code_at_the_end() {
        let packet = auth_packet("bblp", "12345678");
        assert_eq!(packet.len(), 80);
        assert_eq!(&packet[0..4], &0x40u32.to_le_bytes());
        assert_eq!(&packet[4..8], &0x3000u32.to_le_bytes());
        assert_eq!(&packet[8..16], &[0u8; 8]);
        assert_eq!(&packet[16..20], b"bblp");
        assert_eq!(packet[20], 0);
        assert_eq!(&packet[48..56], b"12345678");
        assert_eq!(packet[56], 0);
    }

    #[test]
    fn a_lan_reject_header_is_live_view_off_and_an_empty_one_is_the_camera() {
        let reject = JpegHeader {
            payload_size: 8,
            itrack: LAN_REJECT,
            flags: 0,
        };
        assert_eq!(interpret_header(&reject), Err(CameraFault::LanLiveViewOff));
        let off = JpegHeader {
            payload_size: 0,
            itrack: 0,
            flags: 0,
        };
        assert_eq!(interpret_header(&off), Err(CameraFault::CameraDisabled));
    }
}
