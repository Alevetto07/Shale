//! Test support: a printer on loopback.
//!
//! Cargo builds every file in `tests/` as its own test binary, so the shared
//! code lives in a subdirectory instead — a module, not a target.

pub mod fake_printer;
