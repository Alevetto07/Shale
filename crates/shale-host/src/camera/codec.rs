//! Cisco's official OpenH264 binary, fetched on first use.
//!
//! The MPEG-LA (now Via LA) promise covers that binary, and only when a
//! product downloads it from Cisco rather than compiling it. A release build
//! does this: the file lands in the app data directory, its SHA-256 has to be
//! one the OpenH264 bindings already trust, and the library is loaded with
//! `OpenH264API::from_blob_path`. The `openh264-source` feature compiles the
//! BSD-2 sources instead, and it is off unless someone asks for it.
//!
//! Tests pass a loopback URL or a buffer. They do not request
//! `ciscobinary.openh264.org`.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::cloud::http;

/// The sentence Cisco's binary licence requires in the product.
pub const CREDIT: &str = "OpenH264 Video Codec provided by Cisco Systems, Inc.";

/// One published binary. The hash is the uncompressed library, which is what
/// `from_blob_path` checks against its own list for OpenH264 2.6.0.
pub struct CiscoAsset {
    pub file_name: &'static str,
    pub sha256: &'static str,
}

/// The binary for the machine this build is running on.
pub fn asset_for_host() -> Result<CiscoAsset, String> {
    let asset = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => CiscoAsset {
            file_name: "libopenh264-2.6.0-linux64.8.so",
            sha256: "2f0cde7c6a6abcf5cae76942894ea42897fa677bce4ed6c91a24dd1b041d5f04",
        },
        ("linux", "aarch64") => CiscoAsset {
            file_name: "libopenh264-2.6.0-linux-arm64.8.so",
            sha256: "12e7b33623667cdab0e575170c147b1b36eadb77d0d2aa7ceb5afd3e58902140",
        },
        ("windows", "x86_64") => CiscoAsset {
            file_name: "openh264-2.6.0-win64.dll",
            sha256: "2076cb5675ec6c1a4c70e7a2a322552f547b6eeed649d6dfcd9e02a543b24691",
        },
        ("windows", "aarch64") => CiscoAsset {
            file_name: "openh264-2.6.0-win-arm64.dll",
            sha256: "fb75103938f4f47d119b983e06334df41a803bc72fb5c46e3623f6fea5782732",
        },
        ("macos", "aarch64") => CiscoAsset {
            file_name: "libopenh264-2.6.0-mac-arm64.dylib",
            sha256: "052e98bfcf7a9167d22f3bbb3f5988ef79065591f36af8b52924b22b13624551",
        },
        ("macos", "x86_64") => CiscoAsset {
            file_name: "libopenh264-2.6.0-mac-x64.dylib",
            sha256: "e3dc8bc01fe69363f61fd3c02fd27798537a585eadd38cd808f303d1ee505a19",
        },
        (os, arch) => {
            return Err(format!(
                "Cisco does not publish an OpenH264 binary Shale knows for {os} on {arch}."
            ));
        }
    };
    Ok(asset)
}

/// Cisco publishes the library as a `.bz2` of the file named above.
pub fn download_url(asset: &CiscoAsset) -> String {
    format!("http://ciscobinary.openh264.org/{}.bz2", asset.file_name)
}

pub fn codec_from_source() -> bool {
    cfg!(feature = "openh264-source")
}

pub fn library_path(dir: &Path) -> Result<PathBuf, String> {
    Ok(dir.join(asset_for_host()?.file_name))
}

/// A verified library, when this build loads one. A from-source build returns
/// nothing: the decoder does not need a file.
pub fn installed_library(dir: &Path) -> Option<PathBuf> {
    if codec_from_source() {
        return None;
    }
    let asset = asset_for_host().ok()?;
    let path = dir.join(asset.file_name);
    if is_verified(&path, asset.sha256) {
        Some(path)
    } else {
        None
    }
}

pub fn is_verified(path: &Path, expected_sha256: &str) -> bool {
    refuse_catalog_cache(path).is_ok()
        && file_sha256(path)
            .ok()
            .is_some_and(|sum| sum.eq_ignore_ascii_case(expected_sha256))
}

pub fn refuse_catalog_cache(path: &Path) -> Result<(), String> {
    let hit = path.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase()
            == "catalog-cache"
    });
    if hit {
        Err(
            "That path is the catalogue cache. Shale keeps downloads and failure frames in the app data directory."
                .into(),
        )
    } else {
        Ok(())
    }
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    Ok(hex_sha256(&bytes))
}

fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Unpack a `.bz2` buffer and keep the library only when the SHA-256 matches.
pub fn install_blob(compressed: &[u8], dest: &Path, expected_sha256: &str) -> Result<(), String> {
    refuse_catalog_cache(dest)?;
    let mut decoder = bzip2::read::BzDecoder::new(compressed);
    let mut plain = Vec::new();
    decoder
        .read_to_end(&mut plain)
        .map_err(|_| "The OpenH264 download could not be unpacked.".to_string())?;
    if plain.is_empty() {
        return Err("The OpenH264 download was empty.".into());
    }
    let sum = hex_sha256(&plain);
    if !sum.eq_ignore_ascii_case(expected_sha256) {
        return Err(
            "The OpenH264 download did not match Cisco's checksum, so it was discarded.".into(),
        );
    }
    write_verified(dest, &plain)
}

/// Download `url`, unpack it, and keep the library only when the SHA-256 matches.
///
/// `url` is whatever the caller asks for. The window passes Cisco's URL. A
/// test passes a loopback URL, so CI does not touch Cisco.
/// Download Cisco's binary into `root/openh264` when it is not there yet.
///
/// A from-source build returns nothing: the decoder does not need a file.
/// `url` overrides Cisco's address. Tests pass a loopback URL. The call with
/// no override is what MCP uses, and the library's own tests cannot reach it.
pub fn prepare_library(root: &Path) -> Result<Option<PathBuf>, String> {
    #[cfg(test)]
    {
        let _ = root;
        Err("Tests do not download Cisco's OpenH264 binary.".into())
    }
    #[cfg(not(test))]
    {
        let override_url = std::env::var("SHALE_OPENH264_URL").ok();
        prepare_library_from(root, override_url.as_deref(), |_, _| {})
    }
}

pub fn prepare_library_from(
    root: &Path,
    url: Option<&str>,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<Option<PathBuf>, String> {
    refuse_catalog_cache(root)?;
    if codec_from_source() {
        return Ok(None);
    }
    let dir = root.join("openh264");
    if let Some(path) = installed_library(&dir) {
        return Ok(Some(path));
    }
    let asset = asset_for_host()?;
    let dest = dir.join(asset.file_name);
    let official = download_url(&asset);
    let fetch_url = url.unwrap_or(official.as_str());
    fetch(fetch_url, &dest, asset.sha256, &mut progress)?;
    installed_library(&dir).map(Some).ok_or_else(|| {
        "Cisco's OpenH264 library did not match the checksum after it was unpacked.".into()
    })
}

pub fn fetch(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    refuse_catalog_cache(dest)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let partial = dest.with_extension("bz2");
    let mut file = File::create(&partial).map_err(|err| err.to_string())?;
    let received =
        http::download_to(url, &mut file, &mut progress).map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())?;
    drop(file);
    if received == 0 {
        let _ = fs::remove_file(&partial);
        return Err("The OpenH264 download was empty.".into());
    }
    let compressed = fs::read(&partial).map_err(|err| err.to_string())?;
    let outcome = install_blob(&compressed, dest, expected_sha256);
    let _ = fs::remove_file(&partial);
    outcome
}

fn write_verified(dest: &Path, plain: &[u8]) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let partial = dest.with_extension("partial");
    let mut file = File::create(&partial).map_err(|err| err.to_string())?;
    file.write_all(plain).map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())?;
    drop(file);
    fs::rename(&partial, dest).map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn the_credit_is_ciscos_required_sentence() {
        assert_eq!(
            CREDIT,
            "OpenH264 Video Codec provided by Cisco Systems, Inc."
        );
    }

    #[test]
    fn the_release_url_names_cisco_and_this_test_does_not_fetch_it() {
        let asset = asset_for_host().expect("platform");
        let url = download_url(&asset);
        assert!(url.starts_with("http://ciscobinary.openh264.org/"), "{url}");
        assert!(url.contains(asset.file_name), "{url}");
        assert!(url.ends_with(".bz2"), "{url}");
        assert_eq!(asset.sha256.len(), 64);
    }

    #[test]
    fn the_catalogue_cache_is_refused_before_any_download() {
        let path = std::env::temp_dir()
            .join("catalog-cache")
            .join("libopenh264.so");
        let err = install_blob(b"not-a-codec", &path, "abc").unwrap_err();
        assert!(err.contains("catalogue cache"), "{err}");
        assert!(!path.exists());
    }

    #[test]
    fn a_blob_is_kept_only_when_the_unpacked_sum_matches() {
        let plain = b"not-ciscos-library";
        let sum = hex_sha256(plain);
        let compressed = compress(plain);
        let dir = std::env::temp_dir().join(format!("shale-codec-blob-{}", std::process::id()));
        let dest = dir.join("codec.so");
        let err = install_blob(&compressed, &dest, "0000").unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!dest.exists());
        install_blob(&compressed, &dest, &sum).expect("install");
        assert_eq!(fs::read(&dest).unwrap(), plain);
        assert!(is_verified(&dest, &sum));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_loopback_download_is_not_cisco() {
        let plain = b"loopback-openh264";
        let sum = hex_sha256(plain);
        let body = compress(plain);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let payload = body.clone();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut header = Vec::new();
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).ok() == Some(1) {
                header.push(byte[0]);
                if header.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&payload);
        });
        let dir = std::env::temp_dir().join(format!("shale-codec-http-{port}"));
        let dest = dir.join("codec.so");
        let url = format!("http://127.0.0.1:{port}/libopenh264.bz2");
        assert!(!url.contains("ciscobinary"));
        assert!(!url.contains("openh264.org"));
        fetch(&url, &dest, &sum, |_, _| {}).expect("fetch");
        assert_eq!(fs::read(&dest).unwrap(), plain);
        let _ = fs::remove_dir_all(&dir);
        let _ = body;
    }

    #[test]
    fn prepare_refuses_the_catalogue_cache_without_connecting() {
        let root = std::env::temp_dir().join("catalog-cache");
        let err = prepare_library_from(&root, Some("http://127.0.0.1:1/nope"), |_, _| {}).unwrap_err();
        assert!(err.contains("catalogue cache"), "{err}");
    }

    #[test]
    fn the_default_prepare_does_not_download_cisco() {
        let root = std::env::temp_dir().join(format!("shale-no-cisco-{}", std::process::id()));
        let err = prepare_library(&root).unwrap_err();
        assert!(err.contains("do not download"), "{err}");
        assert!(!root.join("openh264").exists());
    }

    #[test]
    fn prepare_uses_the_supplied_url_and_discards_a_bad_checksum() {
        let plain = b"not-the-cisco-library";
        let body = compress(plain);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let payload = body.clone();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut header = Vec::new();
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).ok() == Some(1) {
                header.push(byte[0]);
                if header.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&payload);
        });
        let root = std::env::temp_dir().join(format!("shale-prepare-{port}"));
        let url = format!("http://127.0.0.1:{port}/lib.bz2");
        assert!(!url.contains("ciscobinary"));
        assert!(!url.contains("openh264.org"));
        let err = prepare_library_from(&root, Some(&url), |received, total| {
            let _ = (received, total);
        })
        .unwrap_err();
        assert!(err.contains("checksum") || err.contains("did not match"), "{err}");
        assert!(installed_library(&root.join("openh264")).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    fn compress(plain: &[u8]) -> Vec<u8> {
        use bzip2::write::BzEncoder;
        use bzip2::Compression;
        let mut encoder = BzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(plain).unwrap();
        encoder.finish().unwrap()
    }
}
