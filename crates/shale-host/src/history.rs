//! Prints this computer has sent.
//!
//! Cloud tasks live on Bambu's account. This file is the other half: a send
//! that left this machine, kept beside the app's own config and never in the
//! profile cache. The list is short on purpose. It is a record of what went
//! out, not a second job queue.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// How many sends to keep. Older ones fall off the end.
const KEEP: usize = 40;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocalSend {
    pub at_ms: i64,
    pub name: String,
    pub bed_type: String,
    pub remote: String,
    pub bytes: u64,
}

/// Reads the list. A missing file, or one this version cannot read, is an
/// empty history rather than an error the pane has to explain.
pub fn read(path: &Path) -> Vec<LocalSend> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Puts a send at the front and drops anything past [`KEEP`].
pub fn append(path: &Path, entry: LocalSend) -> Result<(), String> {
    let mut rows = read(path);
    rows.insert(0, entry);
    if rows.len() > KEEP {
        rows.truncate(KEEP);
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
    }
    let text = serde_json::to_string_pretty(&rows).map_err(|err| err.to_string())?;
    fs::write(path, text).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_an_empty_history() {
        let path = std::env::temp_dir().join(format!(
            "shale-history-missing-{}-{}.json",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_file(&path);
        assert!(read(&path).is_empty());
    }

    #[test]
    fn the_newest_send_is_first_and_the_list_stops_at_forty() {
        let path = std::env::temp_dir().join(format!(
            "shale-history-{}-{}.json",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_file(&path);
        for index in 0..45 {
            append(
                &path,
                LocalSend {
                    at_ms: index,
                    name: format!("plate {index}"),
                    bed_type: "textured_plate".into(),
                    remote: format!("/cache/{index}.gcode"),
                    bytes: 10,
                },
            )
            .unwrap();
        }
        let rows = read(&path);
        assert_eq!(rows.len(), 40);
        assert_eq!(rows[0].name, "plate 44");
        assert_eq!(rows[39].name, "plate 5");
        let _ = fs::remove_file(&path);
    }

    fn unique() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    }
}
