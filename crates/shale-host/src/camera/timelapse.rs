//! A local recording of the chamber camera.
//!
//! Frames are JPEGs the caller already has. They are written as a numbered
//! sequence, plus an animated GIF preview. When Cisco's OpenH264 library is
//! installed, the same frames are also encoded and wrapped in an MP4. Nothing
//! here talks to a printer.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, ImageReader, RgbaImage};
#[cfg(test)]
use image::codecs::jpeg::JpegEncoder;
#[cfg(test)]
use image::{ExtendedColorType, ImageEncoder};
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;

use super::codec::refuse_catalog_cache;
use super::h264::nal_payloads;

const MAX_FRAMES: usize = 180;
const PREVIEW_WIDTH: u32 = 320;

pub struct TimelapseSaved {
    pub frames: usize,
    pub directory: PathBuf,
    pub preview: PathBuf,
    pub video: Option<PathBuf>,
    pub note: String,
}

/// Write `jpegs` under `folder`/`name`.
///
/// `library` is Cisco's binary, when it has already been checked. Without it
/// the JPEG sequence and the GIF are still written. The catalogue cache is
/// refused.
pub fn save(
    folder: &Path,
    name: &str,
    jpegs: &[Vec<u8>],
    library: Option<&Path>,
    interval_ms: u32,
) -> Result<TimelapseSaved, String> {
    refuse_catalog_cache(folder)?;
    if jpegs.is_empty() {
        return Err("There are no frames to save.".into());
    }
    let kept = jpegs.len().min(MAX_FRAMES);
    let frames = &jpegs[..kept];
    let dir = folder.join(safe_name(name));
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    for (index, jpeg) in frames.iter().enumerate() {
        if !jpeg.starts_with(&[0xff, 0xd8]) {
            return Err(format!("Frame {} is not a JPEG.", index + 1));
        }
        let path = dir.join(format!("frame-{index:04}.jpg"));
        fs::write(&path, jpeg).map_err(|err| err.to_string())?;
    }
    let pictures = decode_preview(frames)?;
    let preview = dir.join("preview.gif");
    write_gif(&preview, &pictures, interval_ms.max(100))?;
    let mut note = format!(
        "Saved {kept} frames as JPEGs and an animated preview in {}.",
        dir.display()
    );
    if kept < jpegs.len() {
        note.push_str(&format!(" The first {MAX_FRAMES} frames were kept."));
    }
    let video = match encode_mp4(library, &pictures, interval_ms) {
        Ok(bytes) => {
            let path = dir.join("timelapse.mp4");
            fs::write(&path, bytes).map_err(|err| err.to_string())?;
            note.push_str(" An MP4 was written with OpenH264.");
            Some(path)
        }
        Err(err) => {
            note.push(' ');
            note.push_str(&err);
            None
        }
    };
    Ok(TimelapseSaved {
        frames: kept,
        directory: dir,
        preview,
        video,
        note,
    })
}

fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "timelapse".into()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn decode_preview(jpegs: &[Vec<u8>]) -> Result<Vec<image::RgbImage>, String> {
    let mut pictures = Vec::with_capacity(jpegs.len());
    for jpeg in jpegs {
        let image = ImageReader::new(std::io::Cursor::new(jpeg))
            .with_guessed_format()
            .map_err(|err| err.to_string())?
            .decode()
            .map_err(|err| err.to_string())?
            .to_rgb8();
        pictures.push(image);
    }
    let (width, height) = preview_size(pictures[0].width(), pictures[0].height());
    let resized = pictures
        .into_iter()
        .map(|image| {
            image::imageops::resize(&image, width, height, image::imageops::FilterType::Triangle)
        })
        .collect();
    Ok(resized)
}

fn preview_size(width: u32, height: u32) -> (u32, u32) {
    let width = width.max(2);
    let height = height.max(2);
    let (width, height) = if width <= PREVIEW_WIDTH {
        (width, height)
    } else {
        let next = (u64::from(height) * u64::from(PREVIEW_WIDTH) / u64::from(width)).max(2);
        (PREVIEW_WIDTH, next as u32)
    };
    (width & !1, height.max(2) & !1)
}

fn write_gif(path: &Path, frames: &[image::RgbImage], interval_ms: u32) -> Result<(), String> {
    let file = File::create(path).map_err(|err| err.to_string())?;
    let mut encoder = GifEncoder::new_with_speed(file, 10);
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(|err| err.to_string())?;
    let delay = Delay::from_numer_denom_ms(interval_ms, 1);
    for frame in frames {
        let rgba: RgbaImage = image::DynamicImage::ImageRgb8(frame.clone()).to_rgba8();
        encoder
            .encode_frame(Frame::from_parts(rgba, 0, 0, delay))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn encode_mp4(
    library: Option<&Path>,
    frames: &[image::RgbImage],
    interval_ms: u32,
) -> Result<Vec<u8>, String> {
    let api = open_api(library)?;
    let width = frames[0].width();
    let height = frames[0].height();
    let fps = (1000 / interval_ms.max(1)).clamp(1, 30);
    let config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(1_500_000))
        .max_frame_rate(FrameRate::from_hz(fps as f32))
        .intra_frame_period(IntraFramePeriod::from_num_frames(12));
    let mut encoder = Encoder::with_api_config(api, config).map_err(|err| err.to_string())?;
    let mut samples = Vec::with_capacity(frames.len());
    for frame in frames {
        let rgb = openh264::formats::RgbSliceU8::new(
            frame.as_raw(),
            (frame.width() as usize, frame.height() as usize),
        );
        let yuv = YUVBuffer::from_rgb8_source(rgb);
        let stream = encoder.encode(&yuv).map_err(|err| err.to_string())?;
        let bytes = stream.to_vec();
        if !bytes.is_empty() {
            samples.push(bytes);
        }
    }
    if samples.is_empty() {
        return Err("OpenH264 did not encode a frame.".into());
    }
    mux_annex_b(&samples, width, height, fps)
}

fn open_api(library: Option<&Path>) -> Result<OpenH264API, String> {
    if let Some(path) = library {
        return OpenH264API::from_blob_path(path).map_err(|err| {
            format!("Cisco's OpenH264 library was not accepted ({err}).")
        });
    }
    #[cfg(feature = "openh264-source")]
    {
        return Ok(OpenH264API::from_source());
    }
    #[cfg(not(feature = "openh264-source"))]
    {
        Err(
            "The MP4 needs Cisco's OpenH264 library. The JPEG sequence and the preview were saved."
                .into(),
        )
    }
}

/// Wrap Annex-B access units in a single-track MP4.
pub(crate) fn mux_annex_b(
    samples: &[Vec<u8>],
    width: u32,
    height: u32,
    fps: u32,
) -> Result<Vec<u8>, String> {
    let mut sps = None;
    let mut pps = None;
    let mut avcc_samples = Vec::new();
    for sample in samples {
        let mut avcc = Vec::new();
        for payload in nal_payloads(sample) {
            if payload.is_empty() {
                continue;
            }
            let kind = payload[0] & 0x1f;
            if kind == 7 && sps.is_none() {
                sps = Some(payload.to_vec());
            } else if kind == 8 && pps.is_none() {
                pps = Some(payload.to_vec());
            }
            if kind == 1 || kind == 5 {
                avcc.extend_from_slice(&(payload.len() as u32).to_be_bytes());
                avcc.extend_from_slice(payload);
            }
        }
        if !avcc.is_empty() {
            avcc_samples.push(avcc);
        }
    }
    let sps = sps.ok_or("The encoder did not produce a parameter set.")?;
    let pps = pps.ok_or("The encoder did not produce a parameter set.")?;
    if sps.len() < 4 {
        return Err("The encoder's parameter set was too short.".into());
    }
    if avcc_samples.is_empty() {
        return Err("The encoder did not produce a picture.".into());
    }
    let fps = fps.max(1);
    let timescale = 1000u32;
    let delta = (timescale / fps).max(1);
    let count = avcc_samples.len() as u32;
    let duration = count.saturating_mul(delta);
    let mut payload = Vec::new();
    for sample in &avcc_samples {
        payload.extend_from_slice(sample);
    }
    let ftyp = box_of(
        b"ftyp",
        &[
            b"isom".as_slice(),
            &0x200u32.to_be_bytes(),
            b"isom",
            b"iso2",
            b"avc1",
            b"mp41",
        ]
        .concat(),
    );
    let mdat = box_of(b"mdat", &payload);
    let chunk_offset = (ftyp.len() + 8) as u32;
    let avcc = avc_decoder_config(&sps, &pps);
    let avc1 = avc1_box(width, height, &box_of(b"avcC", &avcc));
    let stsd = full_box(b"stsd", 0, 0, &join(&[&1u32.to_be_bytes(), avc1.as_slice()]));
    let count_be = count.to_be_bytes();
    let delta_be = delta.to_be_bytes();
    let one = 1u32.to_be_bytes();
    let stts = full_box(b"stts", 0, 0, &join(&[&one, &count_be, &delta_be]));
    let stss = full_box(b"stss", 0, 0, &join(&[&one, &one]));
    let stsc = full_box(b"stsc", 0, 0, &join(&[&one, &one, &count_be, &one]));
    let mut stsz_body = Vec::new();
    stsz_body.extend_from_slice(&0u32.to_be_bytes());
    stsz_body.extend_from_slice(&count.to_be_bytes());
    for sample in &avcc_samples {
        stsz_body.extend_from_slice(&(sample.len() as u32).to_be_bytes());
    }
    let stsz = full_box(b"stsz", 0, 0, &stsz_body);
    let offset_be = chunk_offset.to_be_bytes();
    let stco = full_box(b"stco", 0, 0, &join(&[&one, &offset_be]));
    let stbl = box_of(b"stbl", &join(&[stsd.as_slice(), stts.as_slice(), stss.as_slice(), stsc.as_slice(), stsz.as_slice(), stco.as_slice()]));
    let vmhd = full_box(b"vmhd", 0, 1, &[0u8; 8]);
    let url = full_box(b"url ", 0, 1, &[]);
    let dref = full_box(b"dref", 0, 0, &join(&[&one, url.as_slice()]));
    let dinf = box_of(b"dinf", &dref);
    let minf = box_of(b"minf", &join(&[vmhd.as_slice(), dinf.as_slice(), stbl.as_slice()]));
    let mdhd = full_box(
        b"mdhd",
        0,
        0,
        &[
            &0u32.to_be_bytes() as &[u8],
            &0u32.to_be_bytes(),
            &timescale.to_be_bytes(),
            &duration.to_be_bytes(),
            &0x55c4u16.to_be_bytes(),
            &0u16.to_be_bytes(),
        ]
        .concat(),
    );
    let zero = 0u32.to_be_bytes();
    let hdlr = full_box(
        b"hdlr",
        0,
        0,
        &join(&[&zero, b"vide", &zero, &zero, &zero, b"VideoHandler\0"]),
    );
    let mdia = box_of(b"mdia", &join(&[mdhd.as_slice(), hdlr.as_slice(), minf.as_slice()]));
    let tkhd = full_box(b"tkhd", 0, 7, &track_header(duration, width, height));
    let trak = box_of(b"trak", &join(&[tkhd.as_slice(), mdia.as_slice()]));
    let mvhd = full_box(b"mvhd", 0, 0, &movie_header(timescale, duration));
    let moov = box_of(b"moov", &join(&[mvhd.as_slice(), trak.as_slice()]));
    let mut file = Vec::new();
    file.extend_from_slice(&ftyp);
    file.extend_from_slice(&mdat);
    file.extend_from_slice(&moov);
    Ok(file)
}

fn avc_decoder_config(sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut body = vec![1, sps[1], sps[2], sps[3], 0xff, 0xe1];
    body.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    body.extend_from_slice(sps);
    body.push(1);
    body.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    body.extend_from_slice(pps);
    body
}

fn avc1_box(width: u32, height: u32, avcc: &[u8]) -> Vec<u8> {
    let mut body = vec![0u8; 78];
    body[6] = 0;
    body[7] = 1;
    body[24..26].copy_from_slice(&(width as u16).to_be_bytes());
    body[26..28].copy_from_slice(&(height as u16).to_be_bytes());
    body[28..32].copy_from_slice(&0x0048_0000u32.to_be_bytes());
    body[32..36].copy_from_slice(&0x0048_0000u32.to_be_bytes());
    body[40..42].copy_from_slice(&1u16.to_be_bytes());
    body[74..76].copy_from_slice(&0x0018u16.to_be_bytes());
    body[76..78].copy_from_slice(&(-1i16).to_be_bytes());
    body.extend_from_slice(avcc);
    box_of(b"avc1", &body)
}

fn movie_header(timescale: u32, duration: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&timescale.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    body.extend_from_slice(&0x0100u16.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&matrix());
    body.extend_from_slice(&[0u8; 24]);
    body.extend_from_slice(&2u32.to_be_bytes());
    body
}

fn track_header(duration: u32, width: u32, height: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&1u32.to_be_bytes());
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&duration.to_be_bytes());
    body.extend_from_slice(&[0u8; 8]);
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&matrix());
    body.extend_from_slice(&(width << 16).to_be_bytes());
    body.extend_from_slice(&(height << 16).to_be_bytes());
    body
}

fn matrix() -> Vec<u8> {
    let mut body = Vec::new();
    for value in [
        0x0001_0000u32,
        0,
        0,
        0,
        0x0001_0000,
        0,
        0,
        0,
        0x4000_0000,
    ] {
        body.extend_from_slice(&value.to_be_bytes());
    }
    body
}

fn join(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(part);
    }
    out
}

fn box_of(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = (8 + body.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn full_box(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut head = Vec::with_capacity(4 + body.len());
    head.push(version);
    head.push(((flags >> 16) & 0xff) as u8);
    head.push(((flags >> 8) & 0xff) as u8);
    head.push((flags & 0xff) as u8);
    head.extend_from_slice(body);
    box_of(kind, &head)
}

/// A tiny JPEG, so a test can round-trip a frame without a camera.
#[cfg(test)]
fn solid_jpeg(red: u8, width: u32, height: u32) -> Vec<u8> {
    let mut image = image::RgbImage::new(width, height);
    for pixel in image.pixels_mut() {
        *pixel = image::Rgb([red, 20, 20]);
    }
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 80)
        .write_image(image.as_raw(), width, height, ExtendedColorType::Rgb8)
        .expect("jpeg");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sequence_is_jpegs_and_a_gif_without_a_cisco_download() {
        let folder = std::env::temp_dir().join(format!("shale-lapse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let frames = vec![solid_jpeg(200, 16, 16), solid_jpeg(40, 16, 16)];
        let saved = save(&folder, "cube print", &frames, None, 500).expect("save");
        assert_eq!(saved.frames, 2);
        assert!(saved.directory.join("frame-0000.jpg").is_file());
        assert!(saved.directory.join("frame-0001.jpg").is_file());
        let gif = fs::read(&saved.preview).unwrap();
        assert!(gif.starts_with(b"GIF8"), "{:?}", &gif[..6.min(gif.len())]);
        assert!(saved.video.is_none());
        assert!(saved.note.contains("preview"));
        assert!(!saved.note.contains("ciscobinary"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_catalogue_cache_is_refused() {
        let folder = std::env::temp_dir().join("catalog-cache");
        let err = match save(&folder, "print", &[solid_jpeg(10, 8, 8)], None, 200) {
            Err(err) => err,
            Ok(_) => panic!("the catalogue cache was accepted"),
        };
        assert!(err.contains("catalogue cache"), "{err}");
    }

    /// Run with `SHALE_OPENH264` pointing at Cisco's binary. CI does not download it.
    #[test]
    #[ignore = "needs Cisco's binary at SHALE_OPENH264; CI does not download it"]
    fn a_cisco_library_encodes_an_mp4() {
        let path = std::env::var("SHALE_OPENH264").expect("SHALE_OPENH264");
        let folder = std::env::temp_dir().join(format!("shale-lapse-cisco-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let saved = save(
            &folder,
            "cisco",
            &[solid_jpeg(180, 32, 32), solid_jpeg(40, 32, 32)],
            Some(Path::new(&path)),
            200,
        )
        .expect("save");
        let video = saved.video.expect("mp4");
        let bytes = fs::read(&video).unwrap();
        assert!(bytes.windows(4).any(|mark| mark == b"ftyp"), "not an mp4");
        assert!(saved.note.contains("MP4"));
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_mp4_of_annex_b_names_the_codec_boxes() {
        let sample = vec![
            0, 0, 0, 1, 0x67, 0x42, 0x00, 0x1e, 0x00, 0, 0, 0, 1, 0x68, 0xce, 0x01, 0, 0, 0, 1,
            0x65, 0x88, 0x84, 0x11,
        ];
        let file = mux_annex_b(&[sample], 32, 32, 10).expect("mux");
        assert_eq!(&file[4..8], b"ftyp");
        assert!(file.windows(4).any(|mark| mark == b"mdat"));
        assert!(file.windows(4).any(|mark| mark == b"moov"));
        assert!(file.windows(4).any(|mark| mark == b"avc1"));
        assert!(file.windows(4).any(|mark| mark == b"avcC"));
        assert!(file.windows(4).any(|mark| mark == [0x67, 0x42, 0x00, 0x1e]));
    }
}
