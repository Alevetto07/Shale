//! End-to-end proof of the host stack against `fake_printer.py`.
//!
//! Run the fake first (it listens on 127.0.0.1:8883 and :990), then:
//! `cargo run -p shale-host --example prove_fake`

use std::time::{Duration, Instant};

fn main() {
    let config = shale_host::HostConfig {
        host: "127.0.0.1".into(),
        serial: "FAKE01A000000001".into(),
        code: "12345678".into(),
        bed_type: "textured_plate".into(),
        remote_dir: "cache".into(),
    };

    let link = shale_host::Link::spawn(config.clone());

    // 1. The link connects and the fake sends a state report unprompted.
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut first = link.status();
    while Instant::now() < deadline && first.reports == 0 {
        std::thread::sleep(Duration::from_millis(150));
        first = link.status();
    }
    println!(
        "1. connected={} state={} nozzle={} bed={} reports={} note={:?}",
        first.connected, first.state, first.nozzle, first.bed, first.reports, first.note
    );
    assert!(first.connected, "the link should be connected");
    assert_eq!(first.state, "IDLE");
    assert!(first.reports >= 1);

    // 2. Upload a real file over implicit FTPS.
    let scratch = std::env::temp_dir().join("shale").join("host-proof.gcode");
    std::fs::create_dir_all(scratch.parent().unwrap()).unwrap();
    std::fs::write(
        &scratch,
        "; Shale host proof\nG28\nG1 X10 Y10 Z0.2 F3000\nG1 X20 E0.5\nM104 S0\n",
    )
    .unwrap();
    let bytes = shale_host::upload(&config, &scratch, "host-proof.gcode").expect("upload");
    println!("2. uploaded {bytes} bytes as /cache/host-proof.gcode");
    assert!(bytes > 40);

    // 3. Hand the machine the print command, then read the state it reports.
    let command = shale_host::gcode_file_command(1, "/cache/host-proof.gcode", "textured_plate");
    println!("3. command {}", serde_json::to_string(&command).unwrap());
    link.send(command).expect("send");

    // The machine reports RUNNING first and a percentage a moment later; wait
    // for both rather than reading a zero that means "just started".
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut running = link.status();
    while Instant::now() < deadline && !(running.state == "RUNNING" && running.percent > 0) {
        std::thread::sleep(Duration::from_millis(200));
        running = link.status();
    }
    println!(
        "4. state={} percent={} layer={}/{} file={} nozzle={}/{}",
        running.state, running.percent, running.layer, running.layers, running.file,
        running.nozzle, running.nozzle_target
    );
    assert_eq!(running.state, "RUNNING");
    assert!(running.percent > 0);
    assert!((running.nozzle_target - 220.0).abs() < 1e-9);

    // 4. Pause, and watch the machine say so.
    link.send(shale_host::simple_command(2, "pause")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut paused = link.status();
    while Instant::now() < deadline && paused.state != "PAUSE" {
        std::thread::sleep(Duration::from_millis(200));
        paused = link.status();
    }
    println!("5. after pause: state={} percent={}", paused.state, paused.percent);

    assert_eq!(paused.state, "PAUSE", "the machine should report the pause it was sent");
    println!("6. link closed");
    link.close();
    std::thread::sleep(Duration::from_millis(300));
}
