//! Printer storage, against the loopback FTPS double.
//!
//! Nothing here dials port 990 on a real address. The double binds an ephemeral
//! port on 127.0.0.1, and the test refuses to continue if that is not where it
//! landed.

mod support;

use std::sync::Arc;
use std::time::Duration;

use shale_host::files::{
    delete_file, download_file, list_files, read_file, reprint_archived, reprint_command,
};
use shale_host::{Link, Trust};
use support::fake_printer::{FakePrinter, CERT_A, KEY_A};

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn the_browser_lists_downloads_and_deletes_on_the_loopback_printer() {
    let printer = FakePrinter::start(CERT_A, KEY_A);
    let config = printer.config();
    assert_eq!(
        config.host, "127.0.0.1",
        "a storage test must not leave loopback"
    );
    assert_ne!(
        config.ftps_port, 990,
        "the real printer port is not this double"
    );

    let trust = Trust::new(None);
    let files = list_files(&trust, &config).expect("list");
    let paths: Vec<_> = files.iter().map(|file| file.path.as_str()).collect();
    assert!(paths.contains(&"/cache/bench.gcode"));
    assert!(paths.contains(&"/cache/plate.3mf"));
    assert!(paths.contains(&"/timelapse/bench.mp4"));
    assert!(files.iter().any(|file| file.kind == "timelapse"));
    assert!(files
        .iter()
        .any(|file| file.kind == "gcode" && file.printable));
    assert!(files
        .iter()
        .any(|file| file.kind == "model" && file.printable));

    let dest = std::env::temp_dir().join(format!("shale-storage-{}.mp4", std::process::id()));
    let bytes = download_file(&trust, &config, "/timelapse/bench.mp4", &dest).expect("download");
    assert!(bytes > 0);
    let got = std::fs::read(&dest).expect("saved");
    assert!(got.windows(4).any(|window| window == b"ftyp"));
    let _ = std::fs::remove_file(&dest);

    delete_file(&trust, &config, "/timelapse/bench.mp4").expect("delete");
    let files = list_files(&trust, &config).expect("list again");
    assert!(files.iter().all(|file| file.path != "/timelapse/bench.mp4"));
}

#[test]
fn reprinting_a_file_already_there_sends_no_upload() {
    let printer = FakePrinter::start(CERT_A, KEY_A);
    let config = printer.config();
    assert_eq!(config.host, "127.0.0.1");
    let link = Link::spawn(config.clone());
    assert!(
        printer
            .wait_for(WAIT, || link.status().connected.then_some(()))
            .is_some(),
        "the link never connected: {:?}",
        link.status()
    );
    let before = printer.uploads.lock().unwrap().len();
    link.send(reprint_command(3, "/cache/bench.gcode", "textured_plate"))
        .unwrap();
    let commands = Arc::clone(&printer.commands);
    let command = printer
        .wait_for(WAIT, || commands.lock().unwrap().last().cloned())
        .expect("the reprint never arrived");
    assert_eq!(command["print"]["command"], "gcode_file");
    assert_eq!(command["print"]["param"], "/cache/bench.gcode");
    assert_eq!(
        printer.uploads.lock().unwrap().len(),
        before,
        "reprinting must not upload"
    );

    let mut plate = shale_host::gcode3mf::plate_from_gcode(
        b";TIME:12\nG1 E1\n",
        "bracket",
        "N2S",
        "0.4",
        "PLA",
        "#FFFFFF",
        &[0],
        "textured_plate",
    );
    plate.index = 2;
    let archive = shale_host::gcode3mf::write(&[plate]).expect("3mf");
    let local = std::env::temp_dir().join(format!("shale-reprint-{}.gcode.3mf", std::process::id()));
    std::fs::write(&local, &archive).expect("write");
    shale_host::upload(&config, &local, "job.gcode.3mf").expect("upload");
    let _ = std::fs::remove_file(&local);
    let remote = format!("/{}/job.gcode.3mf", config.remote_dir.trim_matches('/'));
    let trust = Trust::new(None);
    let fetched = read_file(&trust, &config, &remote).expect("read the 3mf back");
    let after_upload = printer.uploads.lock().unwrap().len();
    assert!(after_upload > before, "the sliced file has to land first");
    link.send(reprint_archived(4, &remote, "textured_plate", &fetched).unwrap())
        .unwrap();
    let model = printer
        .wait_for(WAIT, || {
            let guard = commands.lock().unwrap();
            guard
                .iter()
                .rev()
                .find(|command| command["print"]["command"] == "project_file")
                .cloned()
        })
        .expect("the 3MF reprint never arrived");
    assert_eq!(model["print"]["param"], "Metadata/plate_2.gcode");
    assert_eq!(model["print"]["url"], format!("ftp://{remote}"));
    assert_eq!(model["print"]["ams_mapping"], serde_json::json!([0]));
    assert_eq!(
        printer.uploads.lock().unwrap().len(),
        after_upload,
        "reprinting must not upload again"
    );
}
