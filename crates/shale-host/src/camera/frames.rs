//! Failure frames kept on disk, so a warning can be looked at after a restart.
//!
//! The folder is under the app data directory. `SHALE_CACHE_DIR` wins, so a
//! test can point this at a temporary directory and never touch
//! `%APPDATA%\Shale`. A path that contains `catalog-cache` is refused.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::codec::refuse_catalog_cache;

/// How many warnings are kept. Older ones are deleted.
pub const MAX_WARNINGS: usize = 8;
/// How many frames each warning keeps. The newest are the ones that stay.
pub const MAX_FRAMES_EACH: usize = 6;

#[derive(Debug)]
pub struct StoredWarning {
    pub id: String,
    pub note: String,
    pub frames: Vec<Vec<u8>>,
}

/// Where codec files and failure frames live.
///
/// `SHALE_CACHE_DIR` is the override tests set. Without it, this is the same
/// config directory the rest of Shale uses, not the catalogue cache.
pub fn data_root() -> PathBuf {
    data_root_from(&DataEnv::from_process())
}

pub struct DataEnv {
    pub cache_dir: Option<String>,
    pub config_dir: Option<String>,
    pub appdata: Option<String>,
    pub xdg: Option<String>,
    pub home: Option<String>,
    pub macos: bool,
}

impl DataEnv {
    fn from_process() -> Self {
        Self {
            cache_dir: std::env::var("SHALE_CACHE_DIR").ok(),
            config_dir: std::env::var("SHALE_CONFIG_DIR").ok(),
            appdata: std::env::var("APPDATA").ok(),
            xdg: std::env::var("XDG_CONFIG_HOME").ok(),
            home: std::env::var("HOME").ok(),
            macos: cfg!(target_os = "macos"),
        }
    }
}

pub fn data_root_from(env: &DataEnv) -> PathBuf {
    if let Some(dir) = nonempty(env.cache_dir.as_deref()) {
        return PathBuf::from(dir);
    }
    if let Some(dir) = nonempty(env.config_dir.as_deref()) {
        return PathBuf::from(dir);
    }
    if let Some(appdata) = nonempty(env.appdata.as_deref()) {
        return PathBuf::from(appdata).join("dev.shale.desktop");
    }
    if env.macos {
        if let Some(home) = nonempty(env.home.as_deref()) {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("dev.shale.desktop");
        }
    }
    if let Some(xdg) = nonempty(env.xdg.as_deref()) {
        return PathBuf::from(xdg).join("dev.shale.desktop");
    }
    if let Some(home) = nonempty(env.home.as_deref()) {
        return PathBuf::from(home)
            .join(".config")
            .join("dev.shale.desktop");
    }
    std::env::temp_dir().join("shale-config")
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

pub fn warnings_dir(root: &Path) -> PathBuf {
    root.join("failure-frames")
}

/// Keep `note` and the JPEGs. Extra frames are dropped. Older warnings beyond
/// [`MAX_WARNINGS`] are deleted.
pub fn save_warning(root: &Path, note: &str, frames: &[Vec<u8>]) -> Result<StoredWarning, String> {
    refuse_catalog_cache(root)?;
    let kept: Vec<&[u8]> = frames
        .iter()
        .filter(|frame| frame.starts_with(&[0xff, 0xd8]))
        .map(Vec::as_slice)
        .collect();
    if kept.is_empty() {
        return Err("There is no JPEG to keep with this warning.".into());
    }
    let kept = if kept.len() > MAX_FRAMES_EACH {
        &kept[kept.len() - MAX_FRAMES_EACH..]
    } else {
        kept.as_slice()
    };
    let dir = warnings_dir(root);
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let id = format!("{:020}-{}", millis(), dir.read_dir().map(|read| read.count()).unwrap_or(0));
    let dest = dir.join(&id);
    fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
    let mut note_file = File::create(dest.join("note.txt")).map_err(|err| err.to_string())?;
    note_file
        .write_all(note.trim().as_bytes())
        .map_err(|err| err.to_string())?;
    let mut stored = Vec::with_capacity(kept.len());
    for (index, frame) in kept.iter().enumerate() {
        let path = dest.join(format!("frame-{index:02}.jpg"));
        fs::write(&path, frame).map_err(|err| err.to_string())?;
        stored.push(frame.to_vec());
    }
    prune(&dir)?;
    Ok(StoredWarning {
        id,
        note: note.trim().to_string(),
        frames: stored,
    })
}

pub fn list_warnings(root: &Path) -> Result<Vec<StoredWarning>, String> {
    refuse_catalog_cache(root)?;
    let dir = warnings_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        if entry.path().is_dir() {
            names.push(entry.file_name());
        }
    }
    names.sort();
    names.reverse();
    let mut warnings = Vec::new();
    for name in names.into_iter().take(MAX_WARNINGS) {
        let path = dir.join(&name);
        let note = fs::read_to_string(path.join("note.txt")).unwrap_or_default();
        let mut frames = Vec::new();
        for index in 0..MAX_FRAMES_EACH {
            let file = path.join(format!("frame-{index:02}.jpg"));
            if !file.is_file() {
                break;
            }
            let bytes = fs::read(&file).map_err(|err| err.to_string())?;
            if bytes.starts_with(&[0xff, 0xd8]) {
                frames.push(bytes);
            }
        }
        if frames.is_empty() {
            continue;
        }
        warnings.push(StoredWarning {
            id: name.to_string_lossy().into_owned(),
            note: note.trim().to_string(),
            frames,
        });
    }
    Ok(warnings)
}

fn prune(dir: &Path) -> Result<(), String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        if entry.path().is_dir() {
            names.push(entry.file_name());
        }
    }
    names.sort();
    if names.len() <= MAX_WARNINGS {
        return Ok(());
    }
    let extra = names.len() - MAX_WARNINGS;
    for name in names.into_iter().take(extra) {
        let _ = fs::remove_dir_all(dir.join(name));
    }
    Ok(())
}

fn millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    fn jpeg(marker: u8) -> Vec<u8> {
        let mut image = image::RgbImage::new(8, 8);
        for pixel in image.pixels_mut() {
            *pixel = image::Rgb([marker, 10, 10]);
        }
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 80)
            .write_image(
                image.as_raw(),
                8,
                8,
                image::ExtendedColorType::Rgb8,
            )
            .expect("jpeg");
        bytes
    }

    #[test]
    fn shale_cache_dir_wins_over_appdata() {
        let root = data_root_from(&DataEnv {
            cache_dir: Some(r"C:\temp\shale-isolated".into()),
            config_dir: None,
            appdata: Some(r"C:\Users\me\AppData\Roaming".into()),
            xdg: None,
            home: Some(r"C:\Users\me".into()),
            macos: false,
        });
        assert_eq!(root, PathBuf::from(r"C:\temp\shale-isolated"));
        let text = root.to_string_lossy().to_ascii_lowercase();
        assert!(!text.contains("appdata"), "{text}");
        assert!(!text.contains("roaming"), "{text}");
    }

    #[test]
    fn the_catalogue_cache_is_refused_before_any_file_is_written() {
        let root = std::env::temp_dir().join("catalog-cache");
        let err = save_warning(&root, "spaghetti", &[jpeg(200)]).unwrap_err();
        assert!(err.contains("catalogue cache"), "{err}");
        assert!(!warnings_dir(&root).exists());
    }

    #[test]
    fn warnings_are_capped_and_can_be_read_again() {
        let root = std::env::temp_dir().join(format!("shale-frames-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let frame = jpeg(180);
        let mut many = Vec::new();
        for _ in 0..MAX_FRAMES_EACH + 2 {
            many.push(frame.clone());
        }
        for index in 0..MAX_WARNINGS + 3 {
            save_warning(&root, &format!("warning {index}"), &many).expect("save");
        }
        let listed = list_warnings(&root).expect("list");
        assert_eq!(listed.len(), MAX_WARNINGS);
        assert!(listed[0].note.contains("warning"));
        assert_eq!(listed[0].frames.len(), MAX_FRAMES_EACH);
        assert!(listed[0].frames[0].starts_with(&[0xff, 0xd8]));
        assert!(!listed.iter().any(|item| item.note == "warning 0"));
        // The temp directory on Windows lives under AppData\Local\Temp. That is
        // not the product folder %APPDATA%\Shale, which this test never opens.
        assert!(
            !root.components().any(|component| {
                let name = component.as_os_str().to_string_lossy();
                name.eq_ignore_ascii_case("Shale") || name.eq_ignore_ascii_case("catalog-cache")
            }),
            "{root:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
