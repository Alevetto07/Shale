//! What happens when the printer on the end of the cable is not the printer
//! that was there last time.
//!
//!     cargo test -p shale-host --test pinning
//!
//! The double is `tests/support/fake_printer.rs`, which can be started with
//! either of two committed certificates. The first sighting of a printer has
//! nothing to compare against, by definition, so what is tested here is that
//! the sighting is *recorded*, that the recorded value is the certificate's
//! SHA-256, and that a different certificate is refused on both channels the
//! host uses — the status link and the upload.

mod support;

use std::time::Duration;

use shale_host::{hex, parse_hex, HostError, Link, Trust};
use support::fake_printer::{
    FakePrinter, CERT_A, CERT_B, FINGERPRINT_A, FINGERPRINT_B, KEY_A, KEY_B,
};

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn the_first_connection_records_the_certificate_it_was_offered() {
    let printer = FakePrinter::start(CERT_A, KEY_A);
    let link = Link::spawn(printer.config());

    let seen = printer.wait_for(WAIT, || link.pin());
    let seen = seen.expect("the handshake recorded a fingerprint");
    assert_eq!(
        hex(&seen),
        FINGERPRINT_A,
        "the pin is the certificate's SHA-256"
    );

    // And the window is told, so the value can be stored against this printer.
    let status = link.status();
    assert_eq!(status.fingerprint, FINGERPRINT_A);
    assert!(status.connected);
}

#[test]
fn the_same_printer_connects_again_under_its_pin() {
    let printer = FakePrinter::start(CERT_A, KEY_A);
    let pin = parse_hex(FINGERPRINT_A).expect("a pin");
    let link = Link::spawn_pinned(printer.config(), Some(pin));

    assert!(
        printer
            .wait_for(WAIT, || link.status().connected.then_some(()))
            .is_some(),
        "a printer with the pinned certificate is let in: {:?}",
        link.status()
    );
}

#[test]
fn a_changed_certificate_stops_the_link_and_says_why() {
    // Something is answering on the printer's address, and it is not the
    // machine that was trusted last time.
    let printer = FakePrinter::start(CERT_B, KEY_B);
    let pin = parse_hex(FINGERPRINT_A).expect("a pin");
    let link = Link::spawn_pinned(printer.config(), Some(pin));

    let note = printer.wait_for(WAIT, || {
        let status = link.status();
        status
            .note
            .contains("certificate changed")
            .then_some(status)
    });
    let status = note.expect("the link reported the refusal");
    assert!(
        !status.connected,
        "a changed certificate is not a connection"
    );
    assert!(
        status.note.contains("certificate changed"),
        "the sentence names the problem: {}",
        status.note
    );
    assert!(
        status.note.contains(FINGERPRINT_A) && status.note.contains(FINGERPRINT_B),
        "both fingerprints are in it, so a person can compare them: {}",
        status.note
    );

    // And it stays refused: this is a question, not a flaky socket.
    std::thread::sleep(Duration::from_millis(300));
    let again = link.status();
    assert!(!again.connected);
    assert!(again.note.contains("certificate changed"));
}

#[test]
fn the_upload_channel_is_guarded_by_the_same_pin() {
    let printer = FakePrinter::start(CERT_B, KEY_B);
    let pin = parse_hex(FINGERPRINT_A).expect("a pin");
    let trust = Trust::new(Some(pin));

    let dir = std::env::temp_dir().join("shale-pinning-upload");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plate.gcode");
    std::fs::write(
        &path,
        ";===== machine: A1 =========================\nM104 S215\n",
    )
    .unwrap();

    let result = shale_host::upload_with(&trust, &printer.config(), &path, "plate.gcode");
    match result {
        Err(HostError::Pin { note }) => {
            assert!(note.contains("certificate changed"), "{note}");
        }
        other => panic!("the upload should have been refused, got {other:?}"),
    }
    assert!(
        printer.uploads().is_empty(),
        "nothing may be sent to a machine that cannot be identified"
    );
}

#[test]
fn a_hex_pin_survives_a_round_trip() {
    let pin = parse_hex(FINGERPRINT_A).unwrap();
    assert_eq!(hex(&pin), FINGERPRINT_A);
    // Stored with colons, as openssl prints it, and still readable.
    let colonised = "61:AC:C0:AE:B5:25:5C:5D:05:48:B5:C3:AF:EF:D8:EC:58:C5:A5:12:A9:67:2D:E9:F4:49:5F:E1:B3:A2:6F:11";
    assert_eq!(
        parse_hex(colonised).map(|pin| hex(&pin)),
        Some(FINGERPRINT_A.to_string())
    );
    assert_eq!(parse_hex("not a fingerprint"), None);
}
