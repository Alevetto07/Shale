//! Turn an Annex-B H.264 access unit into the same JPEG the A1 path stores.
//!
//! A release build loads Cisco's official binary, already checked and stored
//! in the app data directory. The `openh264-source` feature compiles the
//! BSD-2 library instead, and that feature is off in a release build. A
//! packet that is not a picture yet returns nothing. A decoded picture is a
//! JPEG, so the overlay, the detector, the snapshot, and the plate check do
//! not grow a second path.

use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};
use openh264::decoder::{DecodedYUV, Decoder};
#[cfg(not(feature = "openh264-source"))]
use openh264::decoder::DecoderConfig;
use openh264::formats::YUVSource;
#[cfg(not(feature = "openh264-source"))]
use openh264::OpenH264API;

pub struct H264Pictures {
    decoder: Decoder,
}

impl H264Pictures {
    /// Open a decoder.
    ///
    /// `library` is Cisco's binary. A from-source build ignores it.
    pub fn open(library: Option<&Path>) -> Result<Self, String> {
        #[cfg(feature = "openh264-source")]
        {
            let _ = library;
            let decoder = Decoder::new().map_err(|err| err.to_string())?;
            return Ok(Self { decoder });
        }
        #[cfg(not(feature = "openh264-source"))]
        {
            let path = library.ok_or_else(|| {
                "Cisco's OpenH264 library is not installed yet. Shale downloads it on first use."
                    .to_string()
            })?;
            let api = OpenH264API::from_blob_path(path).map_err(|err| {
                format!("Cisco's OpenH264 library was not accepted ({err}).")
            })?;
            let decoder = Decoder::with_api_config(api, DecoderConfig::default())
                .map_err(|err| err.to_string())?;
            Ok(Self { decoder })
        }
    }

    /// Decode one Annex-B unit. `None` means "not a picture yet".
    pub fn push(&mut self, annex_b: &[u8]) -> Option<Vec<u8>> {
        let decoded = self.decoder.decode(annex_b).ok()??;
        jpeg_of(&decoded)
    }
}

fn jpeg_of(yuv: &DecodedYUV<'_>) -> Option<Vec<u8>> {
    let (width, height) = yuv.dimensions();
    let width = u32::try_from(width).ok()?;
    let height = u32::try_from(height).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    let mut rgb = vec![0u8; (width as usize) * (height as usize) * 3];
    yuv.write_rgb8(&mut rgb);
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 80)
        .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
        .ok()?;
    if bytes.starts_with(&[0xff, 0xd8]) {
        Some(bytes)
    } else {
        None
    }
}

/// Annex-B NAL payloads, without the start code, in file order.
pub fn nal_payloads(annex_b: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 < annex_b.len() {
        if annex_b[index..].starts_with(&[0, 0, 0, 1]) {
            starts.push(index + 4);
            index += 4;
        } else if annex_b[index..].starts_with(&[0, 0, 1]) {
            starts.push(index + 3);
            index += 3;
        } else {
            index += 1;
        }
    }
    let mut payloads = Vec::new();
    for (nth, start) in starts.iter().copied().enumerate() {
        let end = match starts.get(nth + 1).copied() {
            Some(next) if next >= 4 && annex_b[next - 4..next] == [0, 0, 0, 1] => next - 4,
            Some(next) if next >= 3 && annex_b[next - 3..next] == [0, 0, 1] => next - 3,
            Some(next) => next,
            None => annex_b.len(),
        };
        if start < end {
            payloads.push(&annex_b[start..end]);
        }
    }
    payloads
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "openh264-source")]
    use image::ImageReader;
    #[cfg(feature = "openh264-source")]
    use std::io::Cursor;

    const RED: &[u8] = include_bytes!("../../tests/fixtures/red-32.h264");

    #[test]
    fn the_red_fixture_is_annex_b() {
        let payloads = nal_payloads(RED);
        assert!(payloads.len() >= 3, "{}", payloads.len());
    }

    #[cfg(feature = "openh264-source")]
    #[test]
    fn a_short_red_stream_becomes_a_red_jpeg() {
        let mut pictures = H264Pictures::open(None).expect("decoder");
        let mut jpeg = None;
        for payload in nal_payloads(RED) {
            let mut unit = vec![0, 0, 0, 1];
            unit.extend_from_slice(payload);
            if let Some(frame) = pictures.push(&unit) {
                jpeg = Some(frame);
            }
        }
        let jpeg = jpeg.expect("a picture");
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        let image = ImageReader::new(Cursor::new(jpeg))
            .with_guessed_format()
            .expect("format")
            .decode()
            .expect("jpeg")
            .to_rgb8();
        assert_eq!(image.dimensions(), (32, 32));
        let pixel = image.get_pixel(16, 16);
        assert!(pixel[0] > 180, "red {}", pixel[0]);
        assert!(pixel[1] < 80, "green {}", pixel[1]);
        assert!(pixel[2] < 80, "blue {}", pixel[2]);
    }

    #[cfg(feature = "openh264-source")]
    #[test]
    fn a_broken_unit_is_not_a_picture() {
        let mut pictures = H264Pictures::open(None).expect("decoder");
        assert!(pictures.push(&[0, 0, 0, 1, 0x65, 0x11, 0x22]).is_none());
    }

    #[cfg(not(feature = "openh264-source"))]
    #[test]
    fn a_file_that_is_not_ciscos_library_is_refused() {
        let dir = std::env::temp_dir().join(format!("shale-h264-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("not-openh264.so");
        std::fs::write(&path, b"this is not the cisco binary").unwrap();
        let err = match H264Pictures::open(Some(&path)) {
            Err(err) => err,
            Ok(_) => panic!("an unknown library was accepted"),
        };
        let text = err.to_lowercase();
        assert!(
            text.contains("openh264") || text.contains("cisco") || text.contains("hash"),
            "{err}"
        );
        let missing = match H264Pictures::open(None) {
            Err(err) => err,
            Ok(_) => panic!("a missing library opened"),
        };
        assert!(missing.contains("not installed"), "{missing}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
