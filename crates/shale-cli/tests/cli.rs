//! What a person sees before they have read any documentation.

use std::process::Command;

use shale_core::integration_test_cache_dir;

fn binary() -> Command {
    let cache = integration_test_cache_dir("shale-cli");
    let mut command = Command::new(env!("CARGO_BIN_EXE_shale-cli"));
    command.env("SHALE_TEST", "1");
    command.env("SHALE_CACHE_DIR", cache);
    command
}

#[test]
fn help_names_every_command_and_environment_variable() {
    let output = binary()
        .arg("--help")
        .output()
        .expect("run shale-cli --help");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "shale info",
        "shale printers",
        "shale settings",
        "shale slice",
    ] {
        assert!(
            text.contains(expected),
            "help is missing `{expected}`:\n{text}"
        );
    }
    for variable in [
        "SHALE_CURA_DIR",
        "SHALE_MATERIALS_DIR",
        "SHALE_ORCA_DIR",
        "SHALE_CURAENGINE",
    ] {
        assert!(text.contains(variable), "help is missing {variable}");
    }
}

#[test]
fn no_arguments_prints_the_usage_rather_than_failing() {
    let output = binary().output().expect("run shale-cli");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("slice a model"));
}

#[test]
fn version_is_the_package_version() {
    let output = binary().arg("--version").output().expect("run --version");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("shale "), "{text}");
    assert!(text.trim().ends_with(env!("CARGO_PKG_VERSION")), "{text}");
}

#[test]
fn an_unknown_command_says_so_and_fails() {
    let output = binary().arg("shred").output().expect("run shred");
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("unknown command `shred`"), "{text}");
    assert!(
        text.contains("shale help"),
        "the error should point at help: {text}"
    );
}

#[test]
fn slice_without_a_model_explains_what_is_missing() {
    let output = binary().arg("slice").output().expect("run slice");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("needs a model path"));
}

#[test]
fn info_never_prints_contradictory_env_lines() {
    let output = binary().arg("info").output().expect("run info");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text.contains("set (not set"),
        "contradictory set/auto line:\n{text}"
    );
    assert!(
        !text.contains("missing — not set"),
        "contradictory missing/auto line:\n{text}"
    );
    assert!(
        !text.contains("missing — missing"),
        "empty not-found hint:\n{text}"
    );
    for line in text.lines() {
        if line.starts_with("SHALE_") {
            assert!(
                line.contains("set: ")
                    || line.contains("not set (auto-detected: ")
                    || line.contains("not set (optional")
                    || line.contains("not found — "),
                "unexpected env line format: {line}"
            );
        }
    }
}