//! The Obico ONNX file, downloaded once into the app data directory.
//!
//! The file is about 193 MB and is not part of the repository. A caller passes
//! the directory. That directory must not be the catalogue cache: a test, and
//! this function, refuse a path that contains `catalog-cache`. The bytes are
//! kept only when the SHA-256 matches the published sum. Tests pass a loopback
//! URL and never request the release.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{OBICO_ONNX_SHA256, OBICO_ONNX_URL};
use crate::cloud::http;

pub const FILE_NAME: &str = "obico-failure.onnx";

pub fn release_url() -> &'static str {
    OBICO_ONNX_URL
}

pub fn model_path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// The catalogue cache is for printer profiles. The model does not belong there.
pub fn refuse_catalog_cache(path: &Path) -> Result<(), String> {
    let hit = path.components().any(|component| {
        component.as_os_str().to_string_lossy().to_ascii_lowercase() == "catalog-cache"
    });
    if hit {
        Err(
            "The failure model is stored in the app data directory, not the catalogue cache."
                .into(),
        )
    } else {
        Ok(())
    }
}

pub fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    Ok(hex_sha256(&bytes))
}

pub fn is_verified(path: &Path) -> bool {
    refuse_catalog_cache(path).is_ok()
        && file_sha256(path)
            .ok()
            .is_some_and(|sum| sum.eq_ignore_ascii_case(OBICO_ONNX_SHA256))
}

/// Download `url` to `dest` and keep it only when the SHA-256 matches.
///
/// `url` is whatever the caller asks for. The window passes the release URL.
/// A test passes a loopback URL and its own expected sum, so CI does not
/// touch the release.
pub fn install(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    expected_len: Option<u64>,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    refuse_catalog_cache(dest)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let partial = dest.with_extension("partial");
    let mut file = File::create(&partial).map_err(|err| err.to_string())?;
    let received = http::download_to(url, &mut file, &mut progress).map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())?;
    drop(file);
    if let Some(expected) = expected_len {
        if received != expected {
            let _ = fs::remove_file(&partial);
            return Err(format!(
                "The download is {received} bytes, and the published file is {expected} bytes."
            ));
        }
    }
    let sum = file_sha256(&partial)?;
    if !sum.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(&partial);
        return Err(
            "The download does not match the published checksum, so it was discarded.".into(),
        );
    }
    fs::rename(&partial, dest).map_err(|err| err.to_string())?;
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn the_catalogue_cache_is_refused() {
        let path = std::env::temp_dir().join("catalog-cache").join(FILE_NAME);
        let err = install("http://127.0.0.1:1/nope", &path, "abc", None, |_, _| {}).unwrap_err();
        assert!(err.contains("catalogue cache"), "{err}");
        assert!(!path.exists());
    }

    #[test]
    fn a_loopback_file_is_kept_only_when_the_sum_matches() {
        let body = b"not-the-obico-weights";
        let sum = hex_sha256(body);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for _ in 0..2 {
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
                let request = String::from_utf8_lossy(&header);
                if request.starts_with("GET /jump") {
                    let response = format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/file\r\nContent-Length: 0\r\n\r\n"
                    );
                    let _ = stream.write_all(response.as_bytes());
                    continue;
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(body);
            }
        });

        let dest = std::env::temp_dir().join(format!("shale-model-{port}")).join(FILE_NAME);
        let url = format!("http://127.0.0.1:{port}/jump");
        assert!(!url.contains("github.com"));
        let err = install(&url, &dest, "0000", Some(body.len() as u64), |_, _| {}).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!dest.exists());

        // The redirect server has already answered once. Start another for the good sum.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let good = body.to_vec();
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
                good.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&good);
        });
        let dest = std::env::temp_dir().join(format!("shale-model-ok-{port}")).join(FILE_NAME);
        let url = format!("http://127.0.0.1:{port}/file");
        assert!(!url.contains("github.com"));
        install(&url, &dest, &sum, Some(body.len() as u64), |_, _| {}).expect("install");
        assert!(is_verified_with(&dest, &sum));
        assert!(!dest.to_string_lossy().to_ascii_lowercase().contains("catalog-cache"));
        let _ = fs::remove_dir_all(dest.parent().unwrap());
    }

    fn is_verified_with(path: &Path, sum: &str) -> bool {
        file_sha256(path).ok().is_some_and(|got| got == sum)
    }
}
