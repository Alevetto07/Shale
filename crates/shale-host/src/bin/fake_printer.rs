//! The loopback printer, as something a person can run.
//!
//! The wiring tests start the double inside themselves, which is the right
//! place for a test but no help when what needs a printer is a *window*: the
//! certificate-pin refusal lives in the device tab, and no test binary can put
//! a sentence on that screen. This binary can. It binds the ports a Bambu
//! machine actually uses — 8883 for MQTT over TLS, 990 for implicit FTPS — on
//! 127.0.0.1 and nothing else, prints the identity it is offering, and runs
//! until it is killed.
//!
//! ```text
//! cargo run -p shale-host --features fake-printer --bin shale-fake-printer
//! cargo run -p shale-host --features fake-printer --bin shale-fake-printer -- --cert b
//! cargo run -p shale-host --features fake-printer --bin shale-fake-printer -- --mqtt 0 --ftps 0
//! ```
//!
//! `--mqtt 0` asks the system for a free port and prints it, which is what a
//! script wants when 8883 is already taken. The certificate is either `a` or
//! `b`; the pin test uses one and then the other, and so does a person proving
//! the refusal.
//!
//! The double itself is `tests/support/fake_printer.rs`, included rather than
//! copied: a second machine written for this binary would drift from the one
//! the tests trust.

// The double is written for the tests, which read its commands, uploads and
// wait_for; this binary only needs it to exist and answer a handshake, so
// most of its surface is dead here and alive there.
#[allow(dead_code)]
#[path = "../../tests/support/fake_printer.rs"]
mod fake_printer;

use std::time::Duration;

fn main() {
    let mut mqtt: u16 = 8883;
    let mut ftps: u16 = 990;
    let mut which = "a";

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cert" => {
                which = match args.next().as_deref() {
                    Some("b") => "b",
                    Some("a") | None => "a",
                    Some(other) => {
                        eprintln!("--cert takes a or b, not {other}");
                        std::process::exit(2);
                    }
                }
            }
            "--mqtt" => match args.next().and_then(|value| value.parse().ok()) {
                Some(port) => mqtt = port,
                None => {
                    eprintln!("--mqtt takes a port number");
                    std::process::exit(2);
                }
            },
            "--ftps" => match args.next().and_then(|value| value.parse().ok()) {
                Some(port) => ftps = port,
                None => {
                    eprintln!("--ftps takes a port number");
                    std::process::exit(2);
                }
            },
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }

    let (cert, key, fingerprint) = if which == "b" {
        (
            fake_printer::CERT_B,
            fake_printer::KEY_B,
            fake_printer::FINGERPRINT_B,
        )
    } else {
        (
            fake_printer::CERT_A,
            fake_printer::KEY_A,
            fake_printer::FINGERPRINT_A,
        )
    };

    let printer = fake_printer::FakePrinter::start_on(mqtt, ftps, cert, key);
    println!("ready on 127.0.0.1");
    println!("  mqtt        {}", printer.mqtt_port);
    println!("  ftps        {}", printer.ftps_port);
    println!("  serial      FAKE01A000000001");
    println!("  certificate {which}");
    println!("  fingerprint {fingerprint}");

    loop {
        std::thread::sleep(Duration::from_secs(3_600));
    }
}
