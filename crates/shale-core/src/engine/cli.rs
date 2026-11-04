//! Run an unmodified `CuraEngine slice` process.
//!
//! Resolved settings are written as the `-r` JSON document the engine already
//! accepts (`global`, `extruder.0`, …). Meshes are ordinary STL paths. Shale
//! does not link CuraEngine; the binary is a separate AGPL-3.0 program.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value as Json;

use super::{EngineOutput, EngineProgress, EngineRequest, SlicingEngine};
use crate::error::Error;

#[derive(Clone, Debug)]
pub struct CliEngine {
    pub program: PathBuf,
    /// Directory prepended to the loader path. Set this when the binary was
    /// extracted from the Cura AppImage and needs the bundled `libArcus` and
    /// friends. Leave it empty when `CuraEngine` is on `PATH`.
    pub lib_dir: Option<PathBuf>,
}

impl CliEngine {
    /// `SHALE_CURAENGINE` wins. Otherwise look next to the executable and in an
    /// installed UltiMaker Cura (Windows Program Files, `CuraEngine.exe`).
    /// The library directory defaults to the folder that contains the binary
    /// so Windows can load the DLLs that sit beside `CuraEngine.exe`.
    pub fn from_env() -> Self {
        if let Some(program) = env::var_os("SHALE_CURAENGINE") {
            let program = PathBuf::from(program);
            let lib_dir = env::var_os("SHALE_CURAENGINE_LIB_DIR")
                .map(PathBuf::from)
                .or_else(|| program.parent().map(Path::to_path_buf));
            return Self { program, lib_dir };
        }
        if let Some(found) = find_installed_engine() {
            return found;
        }
        Self {
            program: PathBuf::from("CuraEngine"),
            lib_dir: None,
        }
    }

    pub fn available(&self) -> bool {
        self.probe().is_ok()
    }

    /// Run `CuraEngine help` and keep stderr. A missing DLL or a bad path
    /// shows up here instead of disappearing.
    pub fn probe(&self) -> Result<(), String> {
        let mut command = Command::new(&self.program);
        command
            .arg("help")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = self.program.parent().filter(|dir| dir.is_dir()) {
            command.current_dir(dir);
        }
        apply_lib_dir(&mut command, self.lib_dir.as_deref());
        match command.output() {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                Err(format!(
                    "CuraEngine at {} exited {}.\n{stderr}{stdout}",
                    self.program.display(),
                    output.status
                ))
            }
            Err(err) => Err(format!(
                "could not start CuraEngine at '{}': {err}. On Windows, Shale looks for CuraEngine.exe inside an UltiMaker Cura folder under Program Files. Set SHALE_CURAENGINE to the full path if it lives somewhere else. DLLs are loaded from {}.",
                self.program.display(),
                self.lib_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "the system search path".into())
            )),
        }
    }
}

fn find_installed_engine() -> Option<CliEngine> {
    let mut roots = Vec::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(value) = env::var_os(key) {
            roots.push(PathBuf::from(value));
        }
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Programs"));
    }
    if let Some(dir) = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
    {
        roots.push(dir);
    }
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("ultimaker cura") || lower == "cura" {
                for exe_name in ["CuraEngine.exe", "CuraEngine"] {
                    let program = entry.path().join(exe_name);
                    if program.is_file() {
                        return Some(CliEngine {
                            lib_dir: Some(entry.path()),
                            program,
                        });
                    }
                }
            }
        }
        for exe_name in ["CuraEngine.exe", "CuraEngine"] {
            let program = root.join(exe_name);
            if program.is_file() {
                return Some(CliEngine {
                    lib_dir: Some(root),
                    program,
                });
            }
        }
    }
    None
}

impl SlicingEngine for CliEngine {
    fn slice(
        &self,
        request: &EngineRequest,
        on_progress: &mut dyn FnMut(EngineProgress),
    ) -> Result<EngineOutput, Error> {
        if request.models.is_empty() {
            return Err(Error::Engine("nothing to slice".into()));
        }
        if let Some(parent) = request.output_gcode.parent() {
            fs::create_dir_all(parent)?;
        }
        let resolved_path = request.output_gcode.with_extension("resolved.json");
        fs::write(&resolved_path, resolved_document(request)?)?;

        let mut command = Command::new(&self.program);
        command
            .arg("slice")
            .arg("-p")
            .arg("-o")
            .arg(&request.output_gcode)
            .arg("-r")
            .arg(&resolved_path);
        // `-s` applies to the object just loaded, so per-part overrides go
        // after each `-l`, exactly like the support modifiers below.
        for model in &request.models {
            command.arg("-l").arg(&model.path);
            for (key, value) in &model.settings {
                command.arg("-s").arg(format!("{key}={value}"));
            }
        }
        // `-s` applies to the object just loaded. Support enforcers and
        // blockers are extra meshes, not a second slicer.
        for modifier in &request.modifiers {
            command.arg("-l").arg(&modifier.path);
            for (key, value) in &modifier.settings {
                command.arg("-s").arg(format!("{key}={value}"));
            }
        }
        if let Some(dir) = self.program.parent().filter(|dir| dir.is_dir()) {
            command.current_dir(dir);
        }
        apply_lib_dir(&mut command, self.lib_dir.as_deref());
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|err| {
            Error::Engine(format!(
                "could not start CuraEngine at '{}': {err}. Paths are passed as separate arguments, including paths with spaces. DLLs are loaded from the engine's own folder ({}).",
                self.program.display(),
                self.lib_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "the system search path".into())
            ))
        })?;

        let (tx, rx) = mpsc::channel::<String>();
        if let Some(stdout) = child.stdout.take() {
            let tx = tx.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let tx = tx.clone();
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });
        }
        drop(tx);

        let started = Instant::now();
        let mut fraction = 0.04;
        let mut log: Vec<String> = Vec::new();
        on_progress(EngineProgress::new(fraction, "CuraEngine started"));

        let push_log = |line: String, log: &mut Vec<String>| {
            if log.len() >= 400 {
                log.remove(0);
            }
            log.push(line);
        };

        let status = loop {
            while let Ok(line) = rx.try_recv() {
                if let Some(parsed) = fraction_from_log(&line) {
                    fraction = fraction.max(parsed);
                }
                push_log(line, &mut log);
            }
            if let Some(layers) = count_layers(&request.output_gcode) {
                if request.estimated_layers > 0 {
                    let estimate =
                        (layers as f32 / request.estimated_layers as f32).clamp(0.05, 0.97);
                    fraction = fraction.max(estimate);
                }
            }
            let message = log.last().cloned().unwrap_or_else(|| "Slicing".to_string());
            on_progress(EngineProgress::new(fraction, message));
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(60 * 30) {
                let _ = child.kill();
                return Err(Error::Engine(
                    "CuraEngine ran for more than 30 minutes and was stopped".into(),
                ));
            }
            thread::sleep(Duration::from_millis(200));
        };
        // Reader threads finish after the pipes close. Wait until they drop
        // the sender so the final "Total print time" line is not missed.
        let mut quiet = 0;
        loop {
            match rx.recv_timeout(Duration::from_millis(40)) {
                Ok(line) => {
                    quiet = 0;
                    push_log(line, &mut log);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    quiet += 1;
                    if quiet > 8 {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let log_tail = log.join("\n");
        if !status.success() {
            return Err(Error::Engine(format!(
                "CuraEngine exited with status {status}.\n{log_tail}"
            )));
        }
        if !request.output_gcode.is_file() {
            return Err(Error::Engine(format!(
                "CuraEngine finished without writing {}",
                request.output_gcode.display()
            )));
        }
        let _ = fs::remove_file(&resolved_path);
        on_progress(EngineProgress::new(1.0, "Slicing finished"));
        Ok(EngineOutput {
            gcode_path: request.output_gcode.clone(),
            print_time_seconds: print_time(&log_tail),
            log_tail,
        })
    }
}

fn resolved_document(request: &EngineRequest) -> Result<String, Error> {
    let mut root = serde_json::Map::new();
    root.insert("global".to_string(), map_to_json(&request.global));
    for (index, extruder) in request.extruders.iter().enumerate() {
        root.insert(format!("extruder.{index}"), map_to_json(extruder));
    }
    serde_json::to_string_pretty(&Json::Object(root))
        .map_err(|err| Error::message(format!("could not encode resolved settings: {err}")))
}

fn map_to_json(values: &std::collections::BTreeMap<String, String>) -> Json {
    let mut object = serde_json::Map::new();
    for (key, value) in values {
        object.insert(key.clone(), Json::String(value.clone()));
    }
    Json::Object(object)
}

pub(crate) fn apply_lib_dir(command: &mut Command, lib_dir: Option<&Path>) {
    let Some(lib_dir) = lib_dir else {
        return;
    };
    for key in ["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "PATH"] {
        let mut paths = vec![lib_dir.to_path_buf()];
        if let Some(existing) = env::var_os(key) {
            paths.extend(env::split_paths(&existing));
        }
        if let Ok(joined) = env::join_paths(paths) {
            command.env(key, joined);
        }
    }
}

fn fraction_from_log(line: &str) -> Option<f32> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("starting export") {
        return Some(0.93);
    }
    if let Some(fraction) = layer_fraction(&lower) {
        let (base, span) = if lower.contains("skin") || lower.contains("infill") {
            (0.55, 0.32)
        } else {
            (0.18, 0.36)
        };
        return Some((base + fraction * span).clamp(0.05, 0.92));
    }
    if let Some(index) = lower.find("progress") {
        let tail = &lower[index..];
        let digits: String = tail
            .chars()
            .skip_while(|ch| !ch.is_ascii_digit())
            .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
            .collect();
        let value = digits.parse::<f32>().ok()?;
        let fraction = if value > 1.0 { value / 100.0 } else { value };
        return Some(fraction.clamp(0.0, 0.99));
    }
    None
}

/// `Processing insets for layer 12 of 50` and the same shape for skin/infill.
fn layer_fraction(lower: &str) -> Option<f32> {
    let marker = "layer ";
    let index = lower.find(marker)?;
    let rest = lower[index + marker.len()..].trim();
    let mut words = rest.split_whitespace();
    let current: f32 = words
        .next()?
        .trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .parse()
        .ok()?;
    if words.next() != Some("of") {
        return None;
    }
    let total: f32 = words
        .next()?
        .trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .parse()
        .ok()?;
    if total <= 0.0 {
        return None;
    }
    Some((current / total).clamp(0.0, 1.0))
}

fn count_layers(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    let count = text.matches(";LAYER:").count() as u32;
    if count == 0 {
        None
    } else {
        Some(count)
    }
}

#[cfg(test)]
mod path_tests {
    use std::path::PathBuf;

    #[test]
    fn a_temp_path_with_spaces_stays_one_argument() {
        let output = PathBuf::from(r"C:\Users\Alessio\AppData\Local\Temp\shale files\slice.gcode");
        let resolved = output.with_extension("resolved.json");
        assert_eq!(
            resolved,
            PathBuf::from(r"C:\Users\Alessio\AppData\Local\Temp\shale files\slice.resolved.json")
        );
        assert!(!resolved.to_string_lossy().contains('"'));
    }
}

fn print_time(log: &str) -> Option<f64> {
    for line in log.lines() {
        let Some(rest) = line.split("Total print time:").nth(1) else {
            continue;
        };
        let token = rest.split_whitespace().next()?;
        if let Ok(seconds) = token
            .trim_matches(|ch: char| ch == ',' || ch == 's')
            .parse::<f64>()
        {
            return Some(seconds);
        }
    }
    None
}
