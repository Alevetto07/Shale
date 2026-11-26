//! Run an unmodified `CuraEngine slice` process.
//!
//! Resolved settings are written as the `-r` JSON document the engine already
//! accepts (`global`, `extruder.0`, …). Meshes are ordinary STL paths. Shale
//! does not link CuraEngine; the binary is a separate AGPL-3.0 program.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value as Json;

use super::{
    absolutize_path, EngineOutput, EngineProgress, EngineRequest, ExportSnapshot, SlicingEngine,
};
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
            let lib_dir = lib_dir_for_program(&program);
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
        hide_engine_console(&mut command);
        if std::env::var_os("SHALE_TRACE").is_some() {
            // The engine's own argv, so a fail-fast (0xc0000409) can be read
            // against the flag it choked on instead of guessed at.
            eprintln!("[shale] engine: {command:?}");
        }
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
            Err(err) => Err(engine_start_failure_message(&self.program, self.lib_dir.as_deref(), &err)),
        }
    }
}

fn lib_dir_display(lib_dir: Option<&Path>) -> String {
    lib_dir
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "the system search path".into())
}

fn engine_start_failure_message(program: &Path, lib_dir: Option<&Path>, err: &std::io::Error) -> String {
    let base = format!(
        "could not start CuraEngine at '{}': {err}",
        program.display()
    );
    #[cfg(windows)]
    {
        format!(
            "{base}. Shale looks for CuraEngine.exe inside an UltiMaker Cura folder under Program Files. Set SHALE_CURAENGINE to the full path if it lives somewhere else. DLLs are loaded from {}.",
            lib_dir_display(lib_dir)
        )
    }
    #[cfg(not(windows))]
    {
        format!(
            "{base}. Set SHALE_CURAENGINE to the CuraEngine binary (for example from an UltiMaker Cura install or your package manager). Native libraries are loaded from {}.",
            lib_dir_display(lib_dir)
        )
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
        let output_gcode = absolutize_path(&request.output_gcode)?;
        if let Some(parent) = output_gcode.parent() {
            fs::create_dir_all(parent)?;
        }
        let resolved_path = output_gcode.with_extension("resolved.json");
        let resolved_request = EngineRequest {
            output_gcode: output_gcode.clone(),
            ..request.clone()
        };
        fs::write(&resolved_path, resolved_document(&resolved_request)?)?;

        let mut command = Command::new(&self.program);
        command
            .arg("slice")
            .arg("-p")
            .arg("-o")
            .arg(&output_gcode)
            .arg("-r")
            .arg(&resolved_path);
        // `-s` applies to the object just loaded, so per-part overrides go
        // after each `-l`, exactly like the support modifiers below.
        for model in &request.models {
            // `-e` selects the extruder for the meshes that follow, so it has to
            // come before this mesh's `-l` and be reset by the next mesh's `-e`.
            if let Some(index) = model.extruder {
                command.arg(extruder_flag(index));
            }
            command.arg("-l").arg(absolutize_path(&model.path)?);
            for (key, value) in &model.settings {
                command.arg("-s").arg(format!("{key}={value}"));
            }
        }
        // `-s` applies to the object just loaded. Support enforcers and
        // blockers are extra meshes, not a second slicer.
        for modifier in &request.modifiers {
            if let Some(index) = modifier.extruder {
                command.arg(extruder_flag(index));
            }
            command.arg("-l").arg(absolutize_path(&modifier.path)?);
            for (key, value) in &modifier.settings {
                command.arg("-s").arg(format!("{key}={value}"));
            }
        }
        if let Some(dir) = self.program.parent().filter(|dir| dir.is_dir()) {
            command.current_dir(dir);
        }
        apply_lib_dir(&mut command, self.lib_dir.as_deref());
        hide_engine_console(&mut command);
        die_with_parent(&mut command);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        if std::env::var_os("SHALE_TRACE").is_some() {
            eprintln!("[shale] engine: {command:?}");
            for key in [
                "cool_fan_enabled",
                "cool_fan_speed",
                "cool_fan_speed_min",
                "cool_fan_speed_max",
                "ironing_enabled",
            ] {
                if let Some(value) = request.global.get(key) {
                    eprintln!("[shale] global {key} = {value}");
                }
            }
        }
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
        let mut engine_guard = EngineGuard::adopt(&child);

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
        let mut export = ExportReader::open(&output_gcode);
        let mut export_log = ExportLog::default();
        let mut exporting = false;
        on_progress(EngineProgress::new(fraction, "CuraEngine started"));

        let push_log = |line: String, log: &mut Vec<String>| {
            if log.len() >= 400 {
                log.remove(0);
            }
            log.push(line);
        };

        let status = loop {
            if engine_cancel_requested() {
                let _ = child.kill();
                let _ = child.wait();
                engine_guard.disarm();
                return Err(Error::Engine(
                    "the slice was cancelled and CuraEngine was stopped".into(),
                ));
            }
            while let Ok(line) = rx.try_recv() {
                if line.to_ascii_lowercase().contains("starting export") {
                    exporting = true;
                }
                if let Some(parsed) = fraction_from_log(&line) {
                    fraction = fraction.max(parsed);
                }
                export_log.note(&line);
                push_log(line, &mut log);
            }
            if export.is_none() {
                export = ExportReader::open(&output_gcode);
            }
            // CuraEngine 5.13 keeps five layer plans and a block-buffered
            // ofstream, and its CLI progress callback prints nothing. A slow
            // export such as the aerospike stays at 0 bytes, so the log is
            // what moves the fraction until the file is flushed.
            let file_snapshot = export.as_mut().and_then(|reader| reader.pull());
            let snapshot = export_progress(file_snapshot, &export_log, request.estimated_layers);
            if let Some(snapshot) = snapshot.as_ref() {
                if snapshot.layers_written > 0 {
                    exporting = true;
                }
                let estimated = if snapshot.estimated_layers > 0 {
                    snapshot.estimated_layers
                } else {
                    request.estimated_layers
                };
                if exporting {
                    fraction = fraction.max(export_fraction(snapshot.layers_written, estimated));
                } else if estimated > 0 && snapshot.layers_written > 0 {
                    let estimate =
                        (snapshot.layers_written as f32 / estimated as f32).clamp(0.05, 0.92);
                    fraction = fraction.max(estimate);
                }
            }
            let message = if exporting {
                let written = snapshot
                    .as_ref()
                    .map(|item| item.layers_written)
                    .unwrap_or(0);
                let estimated = snapshot
                    .as_ref()
                    .map(|item| item.estimated_layers)
                    .filter(|count| *count > 0)
                    .unwrap_or(request.estimated_layers);
                if estimated > 0 {
                    format!("Exporting layer {written} of {estimated}")
                } else {
                    format!("Exporting layer {written}")
                }
            } else {
                log.last().cloned().unwrap_or_else(|| "Slicing".to_string())
            };
            let mut progress = EngineProgress::new(fraction, message);
            if let Some(mut snapshot) = snapshot {
                if snapshot.estimated_layers == 0 {
                    snapshot.estimated_layers = request.estimated_layers;
                }
                progress.partial = Some(snapshot);
            }
            on_progress(progress);
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(60 * 30) {
                let _ = child.kill();
                let _ = child.wait();
                engine_guard.disarm();
                return Err(Error::Engine(
                    "CuraEngine ran for more than 30 minutes and was stopped".into(),
                ));
            }
            thread::sleep(Duration::from_millis(200));
        };
        engine_guard.disarm();
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
        if !output_gcode.is_file() {
            return Err(Error::Engine(format!(
                "CuraEngine finished without writing {}",
                output_gcode.display()
            )));
        }
        let _ = fs::remove_file(&resolved_path);
        on_progress(EngineProgress::new(1.0, "Slicing finished"));
        Ok(EngineOutput {
            gcode_path: output_gcode,
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
    serde_json::to_string(&Json::Object(root))
        .map_err(|err| Error::message(format!("could not encode resolved settings: {err}")))
}

fn map_to_json(values: &std::collections::BTreeMap<String, String>) -> Json {
    let mut object = serde_json::Map::new();
    for (key, value) in values {
        object.insert(key.clone(), Json::String(value.clone()));
    }
    Json::Object(object)
}

/// Cura on Linux is often launched via a small shell wrapper that sets
/// `LD_LIBRARY_PATH` before exec-ing the real ELF. Pushing the AppImage tree
/// onto the loader path for that wrapper breaks `/usr/bin/env` in the shebang.
fn lib_dir_for_program(program: &Path) -> Option<PathBuf> {
    if !program_uses_external_lib_dir(program) {
        return None;
    }
    env::var_os("SHALE_CURAENGINE_LIB_DIR")
        .map(PathBuf::from)
        .or_else(|| program.parent().map(Path::to_path_buf))
}

fn program_uses_external_lib_dir(program: &Path) -> bool {
    let Ok(mut file) = fs::File::open(program) else {
        return true;
    };
    let mut header = [0_u8; 4];
    let Ok(n) = file.read(&mut header) else {
        return true;
    };
    if n >= 2 && header[0] == b'#' && header[1] == b'!' {
        return false;
    }
    if n >= 4 && header == *b"\x7fELF" {
        return true;
    }
    // PE (Windows) and Mach-O binaries are native executables, not shell wrappers.
    n >= 2 && header[0] == b'M' && header[1] == b'Z'
}

/// Hide console windows for CuraEngine children when the GUI sets `SHALE_HIDE_ENGINE_CONSOLE`.
#[cfg(windows)]
pub(crate) fn hide_engine_console(command: &mut Command) {
    if std::env::var_os("SHALE_HIDE_ENGINE_CONSOLE")
        .is_some_and(|value| value != "0" && !value.is_empty())
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

#[cfg(not(windows))]
pub(crate) fn hide_engine_console(_command: &mut Command) {}

pub(crate) fn apply_lib_dir(command: &mut Command, lib_dir: Option<&Path>) {
    let Some(lib_dir) = lib_dir else {
        return;
    };
    #[cfg(windows)]
    let keys = ["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "PATH"];
    #[cfg(not(windows))]
    let keys = ["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"];
    for key in keys {
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

/// Reads a G-code file that is still being written.
///
/// A short read at the current end is not the end of the export. The first
/// versions of this reader treated that as finished, so `slice_progress` sat
/// at 0.93 "Starting export…" for the whole write.
struct ExportReader {
    reader: BufReader<std::fs::File>,
    layers: u32,
    elapsed: Option<f64>,
    header_time: Option<f64>,
    filament_mm: f64,
    absolute_e: bool,
    e: f64,
}

impl ExportReader {
    fn open(path: &Path) -> Option<Self> {
        let file = std::fs::OpenOptions::new().read(true).open(path).ok()?;
        Some(Self {
            reader: BufReader::new(file),
            layers: 0,
            elapsed: None,
            header_time: None,
            filament_mm: 0.0,
            absolute_e: true,
            e: 0.0,
        })
    }

    /// Read newly appended lines. `None` means nothing has been written yet.
    fn pull(&mut self) -> Option<ExportSnapshot> {
        let mut line = String::new();
        let mut budget = 8_000u32;
        loop {
            if budget == 0 {
                break;
            }
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    budget -= 1;
                    self.note(&line);
                }
            }
        }
        if self.layers == 0 && self.elapsed.is_none() && self.filament_mm == 0.0 {
            return None;
        }
        Some(ExportSnapshot {
            layers_written: self.layers,
            estimated_layers: 0,
            print_time_seconds: self.header_time.or(self.elapsed),
            filament_metres: self.filament_mm / 1000.0,
        })
    }

    fn note(&mut self, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.contains(";LAYER:") {
            self.layers = self.layers.saturating_add(1);
        }
        if let Some(rest) = trimmed.strip_prefix(";TIME_ELAPSED:") {
            if let Ok(seconds) = rest.trim().parse::<f64>() {
                self.elapsed = Some(self.elapsed.map(|seen| seen.max(seconds)).unwrap_or(seconds));
            }
        }
        if let Some(rest) = trimmed.strip_prefix(";TIME:") {
            if let Ok(seconds) = rest.trim().parse::<f64>() {
                if seconds > 0.0 && seconds != 6666.0 {
                    self.header_time = Some(seconds);
                }
            }
        }
        if trimmed.starts_with("M83") {
            self.absolute_e = false;
        } else if trimmed.starts_with("M82") {
            self.absolute_e = true;
        }
        let code = trimmed.split(';').next().unwrap_or("").trim();
        if code.starts_with("G0") || code.starts_with("G1") {
            if let Some(value) = extrusion_word(code) {
                let delta = if self.absolute_e {
                    let delta = (value - self.e).max(0.0);
                    self.e = value;
                    delta
                } else if value > 0.0 {
                    value
                } else {
                    0.0
                };
                self.filament_mm += delta;
            }
        }
    }
}

fn extrusion_word(line: &str) -> Option<f64> {
    for word in line.split_whitespace() {
        let mut chars = word.chars();
        if chars.next() == Some('E') {
            return chars.as_str().parse().ok();
        }
    }
    None
}

/// Layers named by the CuraEngine log while the G-code file is still empty.
///
/// `Layer export [N]` is 0-based and the seconds on that line are engine
/// compute time, not print time. Print time comes only from `Total print time`
/// or `Print time (s)`.
#[derive(Clone, Debug, Default)]
struct ExportLog {
    layers_reached: u32,
    layer_count: u32,
    print_time_seconds: Option<f64>,
    filament_mm3: Option<f64>,
}

impl ExportLog {
    fn note(&mut self, line: &str) {
        if let Some(index) = bracket_index(line, "layer export [") {
            self.layers_reached = self.layers_reached.max(index.saturating_add(1));
        }
        if let Some(index) = bracket_index(line, "skipped time reporting for layers [") {
            self.layers_reached = self.layers_reached.max(index.saturating_add(1));
        }
        if let Some(count) = number_after(line, "layer count:") {
            if count > 0.0 {
                self.layer_count = count.round() as u32;
            }
        }
        if let Some(seconds) = number_after(line, "print time (s):") {
            if seconds > 0.0 {
                self.print_time_seconds = Some(seconds);
            }
        } else if let Some(seconds) = number_after(line, "total print time:") {
            if seconds > 0.0 {
                self.print_time_seconds = Some(seconds);
            }
        }
        if let Some(index) = line.to_ascii_lowercase().find("filament (mm") {
            if let Some(volume) = number_after(&line[index..], ":") {
                if volume > 0.0 {
                    self.filament_mm3 = Some(volume);
                }
            }
        }
    }
}

fn bracket_index(line: &str, marker: &str) -> Option<u32> {
    let lower = line.to_ascii_lowercase();
    let index = lower.find(marker)?;
    let rest = &line[index + marker.len()..];
    let end = rest.find(']')?;
    max_u32(&rest[..end])
}

fn max_u32(text: &str) -> Option<u32> {
    let mut best = None;
    let mut current = String::new();
    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            if let Ok(value) = current.parse::<u32>() {
                best = Some(best.map(|seen: u32| seen.max(value)).unwrap_or(value));
            }
            current.clear();
        }
    }
    best
}

fn number_after(line: &str, label: &str) -> Option<f64> {
    let lower = line.to_ascii_lowercase();
    let index = lower.find(&label.to_ascii_lowercase())?;
    let rest = &line[index + label.len()..];
    let token = rest.split_whitespace().next()?;
    token
        .trim_matches(|ch: char| ch == ',' || ch == 's' || ch == ':')
        .parse()
        .ok()
}

/// 1.75 mm filament. The log reports volume, and only at the end of the slice.
fn filament_metres_from_mm3(volume_mm3: f64) -> f64 {
    const RADIUS_MM: f64 = 1.75 / 2.0;
    let area = std::f64::consts::PI * RADIUS_MM * RADIUS_MM;
    if volume_mm3 <= 0.0 {
        return 0.0;
    }
    volume_mm3 / area / 1000.0
}

/// Prefer the G-code file once it has bytes. Until then, use the engine log.
fn export_progress(
    file: Option<ExportSnapshot>,
    log: &ExportLog,
    estimated: u32,
) -> Option<ExportSnapshot> {
    let from_log = if log.layers_reached == 0
        && log.print_time_seconds.is_none()
        && log.filament_mm3.is_none()
    {
        None
    } else {
        let estimated_layers = if log.layer_count > 0 {
            log.layer_count
        } else {
            estimated
        };
        Some(ExportSnapshot {
            layers_written: log.layers_reached,
            estimated_layers,
            print_time_seconds: log.print_time_seconds,
            filament_metres: log
                .filament_mm3
                .map(filament_metres_from_mm3)
                .unwrap_or(0.0),
        })
    };
    match (file, from_log) {
        (None, None) => None,
        (Some(file), None) => Some(file),
        (None, Some(from_log)) => Some(from_log),
        (Some(mut file), Some(from_log)) => {
            if from_log.layers_written > file.layers_written {
                file.layers_written = from_log.layers_written;
            }
            if file.print_time_seconds.is_none() {
                file.print_time_seconds = from_log.print_time_seconds;
            }
            if file.filament_metres <= 0.0 {
                file.filament_metres = from_log.filament_metres;
            }
            if file.estimated_layers == 0 {
                file.estimated_layers = if from_log.estimated_layers > 0 {
                    from_log.estimated_layers
                } else {
                    estimated
                };
            }
            Some(file)
        }
    }
}

fn export_fraction(layers: u32, estimated: u32) -> f32 {
    if estimated == 0 {
        return 0.93;
    }
    let ratio = (layers as f32 / estimated as f32).clamp(0.0, 1.0);
    0.93 + 0.06 * ratio
}

static ENGINE_CANCEL: AtomicBool = AtomicBool::new(false);
static ENGINE_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Stop every CuraEngine this process started. A later slice calls
/// [`clear_engine_cancel`] first.
pub fn cancel_engine_children() {
    ENGINE_CANCEL.store(true, Ordering::SeqCst);
    let pids = ENGINE_PIDS
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    for pid in pids {
        terminate_pid(pid);
    }
}

pub fn clear_engine_cancel() {
    ENGINE_CANCEL.store(false, Ordering::SeqCst);
}

pub(crate) fn engine_cancel_requested() -> bool {
    ENGINE_CANCEL.load(Ordering::SeqCst)
}

fn register_engine(pid: u32) {
    if let Ok(mut pids) = ENGINE_PIDS.lock() {
        if !pids.contains(&pid) {
            pids.push(pid);
        }
    }
}

fn forget_engine(pid: u32) {
    if let Ok(mut pids) = ENGINE_PIDS.lock() {
        pids.retain(|existing| *existing != pid);
    }
}

pub(crate) struct EngineGuard {
    pid: u32,
    armed: bool,
}

impl EngineGuard {
    pub(crate) fn adopt(child: &std::process::Child) -> Self {
        let pid = child.id();
        register_engine(pid);
        assign_job(child);
        Self { pid, armed: true }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
        forget_engine(self.pid);
    }
}

impl Drop for EngineGuard {
    fn drop(&mut self) {
        if self.armed {
            terminate_pid(self.pid);
            forget_engine(self.pid);
        }
    }
}

/// On Linux the kernel kills the child if this process dies, including a
/// kill that never runs our shutdown code. That is what left an orphaned
/// CuraEngine writing `mcp-*.gcode` after shale-mcp stopped.
pub(crate) fn die_with_parent(command: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                extern "C" {
                    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
                    fn getppid() -> i32;
                }
                // PR_SET_PDEATHSIG = 1, SIGKILL = 9.
                prctl(1, 9, 0, 0, 0);
                if getppid() == 1 {
                    std::process::exit(1);
                }
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = command;
    }
}

fn assign_job(child: &std::process::Child) {
    #[cfg(windows)]
    {
        adopt_windows_job(child);
    }
    #[cfg(not(windows))]
    {
        let _ = child;
    }
}

#[cfg(windows)]
fn adopt_windows_job(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    static JOB: OnceLock<isize> = OnceLock::new();
    let handle = *JOB.get_or_init(|| {
        unsafe extern "system" {
            fn CreateJobObjectW(attrs: *mut std::ffi::c_void, name: *const u16) -> isize;
            fn SetInformationJobObject(
                job: isize,
                class: i32,
                info: *const u8,
                length: u32,
            ) -> i32;
        }
        unsafe {
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job != 0 {
                // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE at offset 16 of the
                // extended limit struct. Closing the last handle (when this
                // process exits) stops the engines assigned to the job.
                let mut info = [0u8; 144];
                info[16..20].copy_from_slice(&0x2000u32.to_ne_bytes());
                SetInformationJobObject(job, 9, info.as_ptr(), info.len() as u32);
            }
            job
        }
    });
    if handle == 0 {
        return;
    }
    unsafe extern "system" {
        fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
    }
    unsafe {
        AssignProcessToJobObject(handle, child.as_raw_handle() as isize);
    }
}

fn terminate_pid(pid: u32) {
    #[cfg(unix)]
    unsafe {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        kill(pid as i32, 9);
    }
    #[cfg(windows)]
    unsafe {
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
            fn TerminateProcess(handle: isize, code: u32) -> i32;
            fn CloseHandle(handle: isize) -> i32;
        }
        let handle = OpenProcess(0x0001, 0, pid);
        if handle != 0 {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

#[cfg(test)]
mod path_tests {
    use std::path::PathBuf;

    use super::absolutize_path;

    #[test]
    fn a_relative_output_path_becomes_absolute_against_cwd() {
        let dir = tempfile::tempdir().expect("temp dir");
        let previous = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir.path()).expect("chdir");
        let resolved =
            absolutize_path(std::path::Path::new("nested/out.gcode")).expect("absolutize");
        std::env::set_current_dir(previous).ok();
        assert!(resolved.is_absolute(), "{}", resolved.display());
        assert!(
            resolved.ends_with("nested/out.gcode"),
            "{}",
            resolved.display()
        );
    }

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

/// The flag that points the engine at a spool, in one place because its shape
/// is not optional: `-e1` is one argument, and `-e` with `1` beside it makes
/// CuraEngine fail fast (0xc0000409) instead of reading the number.
fn extruder_flag(index: usize) -> String {
    format!("-e{index}")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{extruder_flag, program_uses_external_lib_dir};

    #[test]
    fn shell_engine_wrappers_skip_external_lib_dir() {
        let path = std::env::temp_dir().join("shale_engine_wrapper_test.sh");
        std::fs::write(&path, "#!/bin/bash\nexit 0\n").expect("temp wrapper");
        assert!(!program_uses_external_lib_dir(&path));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn native_engine_bins_use_external_lib_dir() {
        let candidate = if cfg!(windows) {
            Path::new(r"C:\Windows\System32\cmd.exe")
        } else {
            Path::new("/bin/ls")
        };
        assert!(program_uses_external_lib_dir(candidate));
    }

    #[test]
    fn the_extruder_flag_is_one_attached_argument() {
        assert_eq!(extruder_flag(0), "-e0");
        assert_eq!(extruder_flag(1), "-e1");
        assert_eq!(extruder_flag(3), "-e3");
        // Nothing to split on: the number is not a separate argv entry.
        assert!(!extruder_flag(2).contains(' '));
        assert_eq!(extruder_flag(2).len(), 3);
    }
}

#[cfg(test)]
mod export_tests {
    use super::{
        cancel_engine_children, clear_engine_cancel, export_fraction, export_progress, ExportLog,
        ExportReader,
    };
    use std::io::Write;
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn a_growing_file_is_not_finished_at_the_first_end() {
        let dir = std::env::temp_dir().join(format!("shale-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("part.gcode");
        std::fs::write(&path, ";TIME:6666\n").unwrap();
        let mut reader = ExportReader::open(&path).expect("open");
        assert!(reader.pull().is_none(), "a header placeholder is not a result");
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, ";LAYER:0").unwrap();
        writeln!(file, "G1 X10 Y0 E1.5").unwrap();
        writeln!(file, ";TIME_ELAPSED:40").unwrap();
        file.flush().unwrap();
        let snapshot = reader.pull().expect("new bytes");
        assert_eq!(snapshot.layers_written, 1);
        assert_eq!(snapshot.print_time_seconds, Some(40.0));
        assert!((snapshot.filament_metres - 0.0015).abs() < 1e-6);
        writeln!(file, ";LAYER:1").unwrap();
        writeln!(file, ";TIME_ELAPSED:90").unwrap();
        file.flush().unwrap();
        let snapshot = reader.pull().expect("later bytes");
        assert_eq!(snapshot.layers_written, 2);
        assert_eq!(snapshot.print_time_seconds, Some(90.0));
        let fraction = export_fraction(2, 4);
        assert!(fraction > 0.93 && fraction < 0.99, "{fraction}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_gcode_file_still_moves_from_the_engine_log() {
        let dir = std::env::temp_dir().join(format!("shale-export-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.gcode");
        std::fs::write(&path, "").unwrap();
        let mut reader = ExportReader::open(&path).expect("open");
        assert!(
            reader.pull().is_none(),
            "a 0-byte file has no partial stats"
        );
        let mut log = ExportLog::default();
        assert!(export_progress(None, &log, 400).is_none());
        log.note("Starting export...");
        log.note("Skipped time reporting for layers [0...4]");
        log.note("┌ Layer export [4] accomplished in 12.400s");
        let snapshot = export_progress(reader.pull(), &log, 400).expect("layers from the log");
        assert_eq!(snapshot.layers_written, 5);
        assert!(
            snapshot.print_time_seconds.is_none(),
            "engine compute time is not print time"
        );
        let fraction = export_fraction(snapshot.layers_written, 400);
        assert!(fraction > 0.93 && fraction < 0.95, "{fraction}");
        log.note("┌ Layer export [19] accomplished in 40.100s");
        let later = export_progress(None, &log, 400).unwrap();
        assert_eq!(later.layers_written, 20);
        assert!(export_fraction(20, 400) > fraction);
        log.note("Layer count: 400");
        log.note("Print time (s): 12845");
        log.note("Filament (mm^3): 12345");
        let done = export_progress(None, &log, 0).unwrap();
        assert_eq!(done.estimated_layers, 400);
        assert_eq!(done.print_time_seconds, Some(12845.0));
        let expected = 12345.0 / (std::f64::consts::PI * 0.875 * 0.875) / 1000.0;
        assert!((done.filament_metres - expected).abs() < 1e-6);
        let file = super::super::ExportSnapshot {
            layers_written: 30,
            estimated_layers: 0,
            print_time_seconds: Some(90.0),
            filament_metres: 1.2,
        };
        let merged = export_progress(Some(file), &log, 400).unwrap();
        assert_eq!(merged.layers_written, 30);
        assert_eq!(merged.print_time_seconds, Some(90.0));
        assert!((merged.filament_metres - 1.2).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancelling_stops_a_registered_child() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["-n", "40", "127.0.0.1"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("30");
            command
        };
        let mut child = command.spawn().expect("child");
        let _guard = super::EngineGuard::adopt(&child);
        cancel_engine_children();
        let started = Instant::now();
        loop {
            if child.try_wait().expect("wait").is_some() {
                break;
            }
            if started.elapsed() > Duration::from_secs(5) {
                let _ = child.kill();
                panic!("the child was still running after cancel");
            }
            thread::sleep(Duration::from_millis(30));
        }
        clear_engine_cancel();
    }
}
