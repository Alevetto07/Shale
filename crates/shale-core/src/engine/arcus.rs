//! Arcus backend: Shale listens, CuraEngine connects, and the slice is one
//! protobuf `Slice` message.
//!
//! The wire format is libArcus's, reimplemented here rather than linked.
//! libArcus 4.10 (the generation Cura 5.13 ships) is LGPL-3.0. Later libArcus
//! releases moved to AGPL-3.0. Shale does not link either library and does
//! not copy its source. The header is `0x2BAD0100` in network byte order, then
//! the protobuf size, then a FNV-1a type id of `GetTypeName()`, then the
//! protobuf bytes. A `uint32` of zero is a keepalive.
//!
//! [`super::cli::CliEngine`] remains available. Pick one at runtime with
//! `SHALE_ENGINE=cli` or `SHALE_ENGINE=arcus`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::cli::CliEngine;
use super::proto::{self, Incoming, ObjectMesh};
use super::{EngineOutput, EngineProgress, EngineRequest, LiveLayer, SlicingEngine};
use crate::error::Error;
use crate::mesh;

const HEADER: u32 = 0x2BAD_0100;
const KEEPALIVE: u32 = 0;
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub struct ArcusEngine {
    pub program: PathBuf,
    pub lib_dir: Option<PathBuf>,
}

impl ArcusEngine {
    pub fn new(cli: &CliEngine) -> Self {
        Self {
            program: cli.program.clone(),
            lib_dir: cli.lib_dir.clone(),
        }
    }
}

impl SlicingEngine for ArcusEngine {
    fn slice(
        &self,
        request: &EngineRequest,
        on_progress: &mut dyn FnMut(EngineProgress),
    ) -> Result<EngineOutput, Error> {
        if request.models.is_empty() {
            return Err(Error::Engine("nothing to slice".into()));
        }
        if let Some(parent) = request.output_gcode.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|err| Error::Engine(format!("could not listen for CuraEngine: {err}")))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;

        let mut command = Command::new(&self.program);
        command
            .arg("connect")
            .arg(format!("127.0.0.1:{port}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        super::cli::apply_lib_dir(&mut command, self.lib_dir.as_deref());
        let mut child = command.spawn().map_err(|err| {
            Error::Engine(format!(
                "could not start CuraEngine for Arcus at '{}': {err}",
                self.program.display()
            ))
        })?;

        let stream = accept_engine(&listener, &mut child)?;
        // On Windows an accepted socket inherits the listener's non-blocking
        // mode, which also disables the read timeout below. Put the connection
        // back into blocking mode with a short receive timeout.
        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_millis(200)))?;
        let mut stream = stream;

        let objects = objects_from_request(request)?;
        let global: Vec<(String, String)> = request
            .global
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let extruders: Vec<Vec<(String, String)>> = request
            .extruders
            .iter()
            .map(|map| {
                map.iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .collect();
        let payload = proto::slice_message(&global, &extruders, &objects);
        write_message(&mut stream, proto::type_id("cura.proto.Slice"), &payload)?;
        on_progress(EngineProgress::new(0.02, "Arcus connected"));

        let mut gcode = Vec::new();
        let mut log = Vec::new();
        let mut print_time = None;
        let mut finished = false;
        let mut peak = 0.02f32;
        let mut last_keepalive = Instant::now();
        let started = Instant::now();
        let stderr_log = spawn_log(child.stderr.take(), child.stdout.take());

        while started.elapsed() < Duration::from_secs(60 * 30) {
            if let Some(rx) = &stderr_log {
                while let Ok(line) = rx.try_recv() {
                    push_log(&mut log, line.trim_end().to_string());
                }
            }
            if last_keepalive.elapsed() > Duration::from_millis(400) {
                let _ = write_u32(&mut stream, KEEPALIVE);
                last_keepalive = Instant::now();
            }
            match read_u32(&mut stream) {
                Ok(KEEPALIVE) => continue,
                Ok(0xf0f0_f0f0) => break,
                Ok(header) if header == HEADER => {}
                Ok(other) => {
                    return Err(Error::Engine(format!(
                        "Arcus header was {other:#x}, expected {HEADER:#x}.\n{}",
                        log.join("\n")
                    )));
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::TimedOut
                        || err.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    if let Some(status) = child.try_wait()? {
                        if !finished {
                            return Err(Error::Engine(format!(
                                "CuraEngine exited {status} before the slice finished.\n{}",
                                log.join("\n")
                            )));
                        }
                        break;
                    }
                    continue;
                }
                Err(err) => {
                    if finished {
                        break;
                    }
                    return Err(Error::Engine(format!(
                        "Arcus read failed: {err}.\n{}",
                        log.join("\n")
                    )));
                }
            }
            let size = read_u32_retrying(&mut stream)? as usize;
            let type_id = read_u32_retrying(&mut stream)?;
            let mut body = vec![0u8; size];
            read_exact_retrying(&mut stream, &mut body)?;
            match proto::decode(type_id, &body) {
                Incoming::Progress(amount) => {
                    let fraction = if amount > 1.0 { amount / 100.0 } else { amount };
                    peak = peak.max(fraction.clamp(0.0, 0.99));
                    let percent = (peak * 100.0).round();
                    on_progress(EngineProgress::new(peak, format!("Arcus {percent:.0}%")));
                }
                Incoming::Layer { index, z, paths } => {
                    on_progress(EngineProgress {
                        fraction: peak,
                        message: format!("Layer {index}"),
                        layer: Some(LiveLayer { index, z, paths }),
                    });
                }
                Incoming::Gcode(chunk) => gcode.extend(chunk),
                Incoming::PrintTime(seconds) => print_time = Some(seconds),
                Incoming::Finished => {
                    finished = true;
                    break;
                }
                Incoming::Other(id) => {
                    push_log(&mut log, format!("ignored Arcus message {id}"));
                }
            }
        }
        if child.try_wait()?.is_none() {
            let _ = child.kill();
        }
        let status = child.wait();
        if let Some(rx) = &stderr_log {
            while let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
                push_log(&mut log, line.trim_end().to_string());
            }
        }
        if !finished && gcode.is_empty() {
            return Err(Error::Engine(format!(
                "Arcus ended before SlicingFinished ({status:?}).\n{}",
                log.join("\n")
            )));
        }
        if gcode.is_empty() {
            return Err(Error::Engine(format!(
                "Arcus finished without G-code.\n{}",
                log.join("\n")
            )));
        }
        std::fs::write(&request.output_gcode, &gcode)?;
        on_progress(EngineProgress::new(1.0, "Slicing finished"));
        Ok(EngineOutput {
            gcode_path: request.output_gcode.clone(),
            log_tail: log.join("\n"),
            print_time_seconds: print_time,
        })
    }
}

fn objects_from_request(request: &EngineRequest) -> Result<Vec<ObjectMesh>, Error> {
    let mut objects = Vec::new();
    for model in &request.models {
        objects.push(mesh_from_path(&model.path, model.settings.clone())?);
    }
    for modifier in &request.modifiers {
        objects.push(mesh_from_path(&modifier.path, modifier.settings.clone())?);
    }
    Ok(objects)
}

fn mesh_from_path(
    path: &std::path::Path,
    settings: Vec<(String, String)>,
) -> Result<ObjectMesh, Error> {
    let mesh = mesh::load_mesh(path)?;
    // CuraEngine treats `vertices` as a triangle soup: three XYZ floats per
    // face, not an indexed vertex buffer. `indices` are not read.
    let mut vertices = Vec::with_capacity(mesh.triangles.len() * 9);
    for triangle in &mesh.triangles {
        for index in triangle {
            if let Some(vertex) = mesh.vertices.get(*index as usize) {
                vertices.extend(vertex);
            }
        }
    }
    let mut settings = settings;
    if !settings.iter().any(|(key, _)| key == "extruder_nr") {
        settings.push(("extruder_nr".into(), "0".into()));
    }
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("mesh")
        .to_string();
    Ok(ObjectMesh {
        name,
        vertices,
        indices: Vec::new(),
        settings,
    })
}

fn accept_engine(
    listener: &TcpListener,
    child: &mut std::process::Child,
) -> Result<TcpStream, Error> {
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(status) = child.try_wait()? {
                    return Err(Error::Engine(format!(
                        "CuraEngine exited {status} before connecting. The CLI backend is still available (SHALE_ENGINE=cli)."
                    )));
                }
                if started.elapsed() > ACCEPT_TIMEOUT {
                    let _ = child.kill();
                    return Err(Error::Engine(
                        "CuraEngine did not connect over Arcus. Use the CLI backend with SHALE_ENGINE=cli.".into(),
                    ));
                }
                thread::sleep(Duration::from_millis(40));
            }
            Err(err) => return Err(Error::Engine(format!("Arcus accept failed: {err}"))),
        }
    }
}

fn write_message(stream: &mut TcpStream, type_id: u32, body: &[u8]) -> Result<(), Error> {
    write_u32(stream, HEADER)?;
    write_u32(stream, body.len() as u32)?;
    write_u32(stream, type_id)?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn write_u32(stream: &mut TcpStream, value: u32) -> Result<(), Error> {
    stream.write_all(&value.to_be_bytes())?;
    Ok(())
}

fn read_u32(stream: &mut TcpStream) -> std::io::Result<u32> {
    let mut buf = [0u8; 4];
    read_exact(stream, &mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

/// A message body is read after its header has already announced it, so a
/// timeout in the middle must be retried instead of aborting the slice.
fn read_u32_retrying(stream: &mut TcpStream) -> std::io::Result<u32> {
    let mut buf = [0u8; 4];
    read_exact_retrying(stream, &mut buf)?;
    Ok(u32::from_be_bytes(buf))
}

fn read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<()> {
    read_exact_with(stream, buf, 0)
}

fn read_exact_retrying(stream: &mut TcpStream, buf: &mut [u8]) -> std::io::Result<()> {
    read_exact_with(stream, buf, 150)
}

/// `stall_limit` is the number of consecutive 200 ms receive timeouts that are
/// retried before the read fails. Zero means a timeout is returned at once,
/// which is what the header read wants so it can poll the engine.
fn read_exact_with(
    stream: &mut TcpStream,
    buf: &mut [u8],
    stall_limit: u32,
) -> std::io::Result<()> {
    let mut filled = 0;
    let mut stalled = 0u32;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Arcus connection closed",
                ))
            }
            Ok(n) => {
                filled += n;
                stalled = 0;
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                if stalled >= stall_limit {
                    return Err(err);
                }
                stalled += 1;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn spawn_log(
    stderr: Option<std::process::ChildStderr>,
    stdout: Option<std::process::ChildStdout>,
) -> Option<std::sync::mpsc::Receiver<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    for stream in [
        stderr.map(|pipe| -> Box<dyn Read + Send> { Box::new(pipe) }),
        stdout.map(|pipe| -> Box<dyn Read + Send> { Box::new(pipe) }),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                let _ = tx.send(std::mem::take(&mut line));
            }
        });
    }
    Some(rx)
}

fn push_log(log: &mut Vec<String>, line: String) {
    if log.len() >= 400 {
        log.remove(0);
    }
    log.push(line);
}
