//! Test support: a printer on loopback.
//!
//! Cargo builds every file in `tests/` as its own test binary, so the shared
//! code lives in a subdirectory instead — a module, not a target.

// Each test binary drives a different part of the machine — one uploads and
// reads commands, one only shakes hands — so what a given binary does not use
// is not dead code, it is a capability that binary did not need.
#[allow(dead_code)]
pub mod fake_printer;
