//! The printer's own storage, over the same implicit FTPS the upload uses.
//!
//! Port 990, user `bblp`, password the LAN access code. A test points `ftps_port`
//! at a loopback double and never at a machine on the network. Listing, reading
//! and deleting are the whole of this module; starting a file that is already
//! on the printer is an MQTT command, built here and sent by the link.

use std::io::Read;
use std::path::Path;

use rustls::StreamOwned;
use serde::{Deserialize, Serialize};

use crate::{expect, open_data, pasv, send_cmd, HostConfig, HostError, Result, Trust, Wire};

/// One file on the printer. Directories are walked and not listed on their own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    /// `gcode`, `model`, `timelapse`, or `file`.
    pub kind: String,
    /// A G-code file or a 3MF can be printed again without uploading it.
    pub printable: bool,
}

/// Walk the storage a Bambu machine exposes and return the files in it.
pub fn list_files(trust: &Trust, config: &HostConfig) -> Result<Vec<RemoteFile>> {
    let mut wire = logged_in(trust, config)?;
    let mut files = Vec::new();
    let mut pending = vec!["/".to_string()];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(dir) = pending.pop() {
        if !seen.insert(dir.clone()) || files.len() + pending.len() > 400 {
            continue;
        }
        for entry in list_dir(&mut wire, trust, config, &dir)? {
            if entry.dir {
                pending.push(entry.path);
            } else if let Some(file) = remote_file(entry.path, entry.bytes) {
                files.push(file);
            }
        }
    }
    let _ = send_cmd(&mut wire, "QUIT", "221");
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

/// Copy one remote file onto `destination`. Returns how many bytes landed.
pub fn download_file(
    trust: &Trust,
    config: &HostConfig,
    remote: &str,
    destination: &Path,
) -> Result<u64> {
    let remote = clean_path(remote)?;
    let mut wire = logged_in(trust, config)?;
    let bytes = retrieve(&mut wire, trust, config, &remote)?;
    let _ = send_cmd(&mut wire, "QUIT", "221");
    if let Some(parent) = destination.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(destination, &bytes)?;
    Ok(bytes.len() as u64)
}

/// Remove one file. The caller has already asked.
pub fn delete_file(trust: &Trust, config: &HostConfig, remote: &str) -> Result<()> {
    let remote = clean_path(remote)?;
    let mut wire = logged_in(trust, config)?;
    send_cmd(&mut wire, &format!("DELE {remote}"), "250")?;
    let _ = send_cmd(&mut wire, "QUIT", "221");
    Ok(())
}

/// Start a file that is already on the printer. A `.gcode` uses `gcode_file`.
/// A 3MF uses `project_file` and names the plate inside the archive. Nothing
/// here opens a socket: the link publishes the JSON.
pub fn reprint_command(sequence: u64, remote: &str, bed_type: &str) -> serde_json::Value {
    let remote = if remote.starts_with('/') {
        remote.to_string()
    } else {
        format!("/{remote}")
    };
    let lower = remote.to_ascii_lowercase();
    if lower.ends_with(".gcode") && !lower.ends_with(".gcode.3mf") {
        return crate::gcode_file_command(sequence, &remote, bed_type);
    }
    let name = remote.rsplit('/').next().unwrap_or(&remote);
    serde_json::json!({
        "print": {
            "sequence_id": sequence.to_string(),
            "command": "project_file",
            "param": "Metadata/plate_1.gcode",
            "url": format!("ftp://{remote}"),
            "bed_type": bed_type,
            "timelapse": false,
            "bed_levelling": true,
            "flow_cali": false,
            "vibration_cali": false,
            "layer_inspect": false,
            "use_ams": false,
            "subtask_name": name,
        }
    })
}

struct Listed {
    path: String,
    bytes: u64,
    dir: bool,
}

fn logged_in(trust: &Trust, config: &HostConfig) -> Result<Wire> {
    let mut wire = Wire::open(trust, &config.host, config.ftps_port)?;
    expect(&mut wire, "220")?;
    send_cmd(&mut wire, "USER bblp", "331")?;
    send_cmd(&mut wire, &format!("PASS {}", config.code), "230")?;
    send_cmd(&mut wire, "PBSZ 0", "200")?;
    send_cmd(&mut wire, "PROT P", "200")?;
    send_cmd(&mut wire, "TYPE I", "200")?;
    Ok(wire)
}

fn list_dir(wire: &mut Wire, trust: &Trust, config: &HostConfig, dir: &str) -> Result<Vec<Listed>> {
    let address = pasv(wire)?;
    wire.write_all(format!("LIST {dir}\r\n").as_bytes())?;
    let line = wire.read_line()?;
    if line.starts_with("550") {
        return Ok(Vec::new());
    }
    if !line.starts_with("150") {
        return Err(HostError::Bad(format!("LIST {dir}: {line}")));
    }
    let mut data = open_data(trust, &config.host, address)?;
    let bytes = read_to_end(&mut data)?;
    drop(data);
    expect(wire, "226")?;
    let text = String::from_utf8_lossy(&bytes);
    let mut entries = Vec::new();
    for raw in text.lines() {
        if let Some(entry) = parse_list_line(dir, raw) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn retrieve(wire: &mut Wire, trust: &Trust, config: &HostConfig, remote: &str) -> Result<Vec<u8>> {
    let address = pasv(wire)?;
    send_cmd(wire, &format!("RETR {remote}"), "150")?;
    let mut data = open_data(trust, &config.host, address)?;
    let bytes = read_to_end(&mut data)?;
    drop(data);
    expect(wire, "226")?;
    Ok(bytes)
}

fn read_to_end(
    stream: &mut StreamOwned<rustls::ClientConnection, std::net::TcpStream>,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = [0u8; 16_384];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                if out.is_empty() {
                    return Err(HostError::Bad("the printer sent no data".into()));
                }
                break;
            }
            Err(err) => {
                if out.is_empty() {
                    return Err(err.into());
                }
                break;
            }
        }
    }
    Ok(out)
}

fn parse_list_line(dir: &str, raw: &str) -> Option<Listed> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }
    if line.to_ascii_lowercase().contains("type=") {
        return parse_mlsd(dir, line);
    }
    let mode = line.split_whitespace().next()?;
    if !mode.starts_with(['-', 'd', 'l']) {
        return None;
    }
    let mut parts = line.split_whitespace();
    let _mode = parts.next()?;
    let _links = parts.next()?;
    let _owner = parts.next()?;
    let _group = parts.next()?;
    let size: u64 = parts.next()?.parse().unwrap_or(0);
    let _month = parts.next()?;
    let _day = parts.next()?;
    let _time = parts.next()?;
    let name = parts.collect::<Vec<_>>().join(" ");
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(Listed {
        path: join_path(dir, &name),
        bytes: size,
        dir: mode.starts_with('d'),
    })
}

fn parse_mlsd(dir: &str, line: &str) -> Option<Listed> {
    // Facts come first, then a space and the name. The name follows the last fact.
    let (facts, name) = line.rsplit_once(';')?;
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let mut kind = "";
    let mut size = 0u64;
    for fact in facts.split(';') {
        let mut pair = fact.splitn(2, '=');
        let key = pair.next()?.trim();
        let value = pair.next().unwrap_or("").trim();
        if key.eq_ignore_ascii_case("type") {
            kind = value;
        } else if key.eq_ignore_ascii_case("size") {
            size = value.parse().unwrap_or(0);
        }
    }
    let directory = kind.eq_ignore_ascii_case("dir");
    if kind.eq_ignore_ascii_case("cdir") || kind.eq_ignore_ascii_case("pdir") || kind.is_empty() {
        return None;
    }
    Some(Listed {
        path: join_path(dir, name),
        bytes: size,
        dir: directory,
    })
}

fn join_path(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn remote_file(path: String, bytes: u64) -> Option<RemoteFile> {
    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
    if name.is_empty() {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let folder = path.to_ascii_lowercase();
    let kind = if lower.ends_with(".mp4")
        || lower.ends_with(".avi")
        || lower.ends_with(".mov")
        || folder.contains("/timelapse/")
        || folder.contains("/ipcam/")
    {
        "timelapse"
    } else if lower.ends_with(".gcode") && !lower.ends_with(".gcode.3mf") {
        "gcode"
    } else if lower.ends_with(".3mf") || lower.ends_with(".gcode.3mf") {
        "model"
    } else {
        "file"
    };
    let printable = kind == "gcode" || kind == "model";
    Some(RemoteFile {
        path,
        name,
        bytes,
        kind: kind.to_string(),
        printable,
    })
}

fn clean_path(remote: &str) -> Result<String> {
    let remote = remote.trim();
    if remote.is_empty() || remote.contains("..") || remote.contains('\n') || remote.contains('\r')
    {
        return Err(HostError::Bad("that is not a file on the printer".into()));
    }
    if remote.starts_with('/') {
        Ok(remote.to_string())
    } else {
        Ok(format!("/{remote}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_line_becomes_a_file_and_a_directory() {
        let file = parse_list_line(
            "/cache",
            "-rw-r--r-- 1 bblp bblp 1280 Jan 1 00:00 bench.gcode",
        )
        .unwrap();
        assert_eq!(file.path, "/cache/bench.gcode");
        assert!(!file.dir);
        assert_eq!(file.bytes, 1280);
        let dir = parse_list_line("/", "drwxr-xr-x 1 bblp bblp 0 Jan 1 00:00 timelapse").unwrap();
        assert!(dir.dir);
        assert_eq!(dir.path, "/timelapse");
        assert!(parse_list_line("/", "drwxr-xr-x 1 bblp bblp 0 Jan 1 00:00 ..").is_none());
    }

    #[test]
    fn reprinting_names_the_file_and_uploads_nothing() {
        let gcode = reprint_command(4, "/cache/bench.gcode", "textured_plate");
        assert_eq!(gcode["print"]["command"], "gcode_file");
        assert_eq!(gcode["print"]["param"], "/cache/bench.gcode");
        let model = reprint_command(5, "/cache/plate.3mf", "smooth_plate");
        assert_eq!(model["print"]["command"], "project_file");
        assert_eq!(model["print"]["url"], "ftp:///cache/plate.3mf");
        assert_eq!(model["print"]["param"], "Metadata/plate_1.gcode");
        assert_eq!(model["print"]["bed_type"], "smooth_plate");
        assert_eq!(model["print"]["use_ams"], false);
    }

    #[test]
    fn a_path_that_climbs_out_is_refused() {
        assert!(clean_path("/cache/../etc/passwd").is_err());
        assert!(clean_path("").is_err());
        assert_eq!(
            clean_path("cache/bench.gcode").unwrap(),
            "/cache/bench.gcode"
        );
    }
}
