//! The host end to end, against a printer that exists only on loopback.
//!
//! Every claim about MQTT, the FTPS upload, the `;===== machine:` gate and the
//! overnight queue used to rest on a real A1 being reachable and observed by
//! hand. These tests put a machine on 127.0.0.1 and drive the same code the
//! window drives:
//!
//!     cargo test -p shale-host --test wiring
//!
//! The double is in `tests/support/fake_printer.rs`; it binds ephemeral ports,
//! so nothing here needs administrator rights, and it never leaves loopback.

mod support;

use std::time::Duration;

use shale_core::queue::{Machine, QueueRun, RunState};
use shale_host::{gcode_file_command, Link, Status};
use support::fake_printer::{FakePrinter, CERT_A, KEY_A};

const WAIT: Duration = Duration::from_secs(10);

/// A G-code file the way the A1's own profile writes one: the banner first,
/// before any heat or motion.
fn a1_file(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("plate.gcode");
    std::fs::write(
        &path,
        concat!(
            ";===== machine: A1 =========================\n",
            ";A1 0.4 nozzle\n",
            "M620 S0A\n",
            "M104 S215\n",
            "M109 S215\n",
            "M140 S60\n",
            "G28 Z P0 T140\n",
            "G29.1 Z-0.02\n",
            "G1 X10.000 Y10.000 E0.500\n",
        ),
    )
    .unwrap();
    path
}

/// The same plate started by Cura's generic Marlin sequence, with the Bambu
/// banner quoted in a comment late in the file — which is the case the old
/// "does the file contain the banner" rule accepted by mistake.
fn cura_file(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("cura.gcode");
    std::fs::write(
        &path,
        concat!(
            ";FLAVOR:Marlin\n",
            ";TIME:1234\n",
            "M140 S60\n",
            "M104 S210\n",
            "G28 ;Start GCode\n",
            "G1 X0 Y0\n",
            "; note: a Bambu file would open with ;===== machine: A1\n",
        ),
    )
    .unwrap();
    path
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("shale-wiring-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn an_a1_file_uploads_and_the_start_command_arrives() {
    let printer = FakePrinter::start(CERT_A, KEY_A);
    let config = printer.config();
    let dir = temp_dir("upload");

    // The app's order: the file lands first, then the machine is told to print
    // it, so a failed upload cannot leave a print command pointing at nothing.
    let link = Link::spawn(config.clone());
    let connected = printer.wait_for(WAIT, || link.status().connected.then_some(()));
    assert!(connected.is_some(), "the link never connected: {:?}", link.status());

    let path = a1_file(&dir);
    let sent = shale_host::upload(&config, &path, "plate.gcode").expect("the upload");
    assert_eq!(sent, std::fs::metadata(&path).unwrap().len() as usize);

    let landed = printer.wait_for(WAIT, || {
        let uploads = printer.uploads();
        uploads.first().cloned()
    });
    let (name, bytes) = landed.expect("the printer received the file");
    assert_eq!(name, "/cache/plate.gcode");
    assert!(String::from_utf8_lossy(&bytes).starts_with(";===== machine: A1"));
    assert_eq!(bytes.len(), sent, "the whole file arrived");

    link.send(gcode_file_command(1, "/cache/plate.gcode", "textured_plate"))
        .expect("a command to the machine");
    let command = printer.wait_for(WAIT, || {
        printer
            .commands()
            .into_iter()
            .find(|value| value["print"]["command"] == "gcode_file")
    });
    let command = command.expect("the start command arrived");
    assert_eq!(command["print"]["param"], "/cache/plate.gcode");
    assert_eq!(command["print"]["bed_type"], "textured_plate");
}

#[test]
fn a_cura_started_file_is_refused_before_it_is_sent() {
    let dir = temp_dir("gate");
    let a1 = a1_file(&dir);
    let cura = cura_file(&dir);

    assert!(
        shale_host::starts_with_machine_sequence(&a1),
        "the A1's own sequence is the one thing that may be sent to it"
    );
    assert!(
        !shale_host::starts_with_machine_sequence(&cura),
        "a Cura start must not count, even when the banner appears in a later comment"
    );
}

#[test]
fn a_finished_print_parks_the_queue_until_the_plate_is_cleared() {
    let printer = FakePrinter::start(CERT_A, KEY_A);
    let link = Link::spawn(printer.config());
    assert!(
        printer
            .wait_for(WAIT, || link.status().connected.then_some(()))
            .is_some(),
        "the link never connected: {:?}",
        link.status()
    );

    let mut run = QueueRun::new();
    assert_eq!(run.arm().state, RunState::Armed);
    assert!(run.may_start(), "a press authorises one job");
    run.started();

    // What the machine says, over the wire, in its own words.
    printer.say(serde_json::json!({"print": {
        "gcode_state": "RUNNING", "mc_percent": 3, "subtask_name": "plate"
    }}));
    let running = printer.wait_for(WAIT, || {
        let status = link.status();
        (status.state == "RUNNING").then_some(status)
    });
    let status = running.expect("a RUNNING report reached the window");
    assert_eq!(status.file, "plate");
    let machine = Machine::from_report(&status.state).expect("RUNNING is a state the queue knows");
    assert_eq!(run.observe(machine).state, RunState::Printing);

    printer.say(serde_json::json!({"print": {"gcode_state": "FINISH", "mc_percent": 100}}));
    let finished = printer.wait_for(WAIT, || {
        let status = link.status();
        (status.state == "FINISH").then_some(status)
    });
    let status = finished.expect("a FINISH report reached the window");
    let update = run.observe(Machine::from_report(&status.state).unwrap());
    assert_eq!(update.state, RunState::WaitingForPlate);
    assert!(
        update.note.unwrap_or_default().contains("clear the plate"),
        "the operator is told what to do"
    );
    assert!(!run.may_start(), "nothing may go out with a part still on the bed");

    // Only the person in front of the machine can say the bed is clear.
    assert_eq!(run.plate_cleared().state, RunState::Armed);
    assert!(run.may_start());
}

#[test]
fn a_failed_report_stops_the_run() {
    let printer = FakePrinter::start(CERT_A, KEY_A);
    let link = Link::spawn(printer.config());
    assert!(
        printer
            .wait_for(WAIT, || link.status().connected.then_some(()))
            .is_some()
    );

    let mut run = QueueRun::new();
    run.arm();
    run.started();
    printer.say(serde_json::json!({"print": {"gcode_state": "RUNNING"}}));
    printer
        .wait_for(WAIT, || (link.status().state == "RUNNING").then_some(()))
        .expect("a RUNNING report");

    printer.say(serde_json::json!({"print": {
        "gcode_state": "FAILED", "print_error": 83935248
    }}));
    let failed = printer.wait_for(WAIT, || {
        let status = link.status();
        (status.state == "FAILED").then_some(status)
    });
    let status: Status = failed.expect("a FAILED report reached the window");
    assert_ne!(status.error, 0, "the error number is carried, not dropped");

    let update = run.observe(Machine::from_report(&status.state).unwrap());
    assert_eq!(update.state, RunState::Stopped);
    assert!(update.note.unwrap_or_default().contains("failed"));
    assert!(!run.may_start());
}

/// A file that was actually sliced, up to the fake printer.
///
/// The four tests above use files written by hand. This one takes whatever
/// `shale-mcp` or the window produced and puts it through the same gate and the
/// same upload, so the proof is about the slicer's output rather than about a
/// fixture that agrees with the test:
///
///     SHALE_SLICED_GCODE=C:/Users/Alessio/probe/mcp-a1.gcode \
///         cargo test -p shale-host --test wiring a_sliced_file -- --ignored --nocapture
#[test]
#[ignore]
fn a_sliced_file_reaches_the_fake_printer() {
    let Ok(path) = std::env::var("SHALE_SLICED_GCODE") else {
        eprintln!("set SHALE_SLICED_GCODE to a sliced A1 file; nothing to check");
        return;
    };
    let path = std::path::PathBuf::from(path);
    let text = std::fs::read_to_string(&path).expect("the sliced file");

    assert!(
        shale_host::starts_with_machine_sequence(&path),
        "the gate refuses the slicer's own output"
    );
    assert!(
        !text.contains('{'),
        "a placeholder survived the slice"
    );
    let head: String = text.chars().take(8_192).collect();
    for line in [";===== machine: A1", "M1002 gcode_claim_action : 2", "M1002 set_filament_type:PLA"] {
        assert!(head.contains(line), "the A1's own start is missing {line}");
    }
    let skip = text.chars().count().saturating_sub(4_096);
    let tail: String = text.chars().skip(skip).collect();
    assert!(
        tail.contains("lower motor current to 45% power"),
        "the A1's own end sequence is missing"
    );

    let printer = FakePrinter::start(CERT_A, KEY_A);
    let sent = shale_host::upload(&printer.config(), &path, "plate.gcode").expect("the upload");
    let landed = printer.wait_for(WAIT, || printer.uploads().first().cloned());
    let (name, bytes) = landed.expect("the printer received the file");
    assert_eq!(name, "/cache/plate.gcode");
    assert_eq!(bytes.len(), sent, "the whole file arrived");
    assert_eq!(bytes, std::fs::read(&path).unwrap(), "byte for byte");
    println!("{sent} bytes of sliced G-code arrived at the fake printer");
}
