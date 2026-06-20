//! Hermetic end-to-end test: fuzz the bundled mock server (itself, via the hidden `mock`
//! subcommand) and assert that the intentionally-fragile tool's crash is reported.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mcp-fuzz")
}

#[test]
fn fuzzing_the_mock_finds_the_planted_crash() {
    let dir = std::env::temp_dir();
    let json = dir.join(format!("mcp-fuzz-selftest-{}.json", std::process::id()));

    // mcp-fuzz run --stdio --json <f> -- <self> mock
    let status = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--seed",
            "7",
            "--json",
            json.to_str().unwrap(),
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-fuzz");

    // The `fragile` tool exits(101) on hostile input → a HIGH crash finding → exit code 1.
    assert_eq!(
        status.code(),
        Some(1),
        "expected non-zero exit due to crash finding"
    );

    let report = std::fs::read_to_string(&json).expect("json report written");
    let findings: serde_json::Value = serde_json::from_str(&report).unwrap();
    let arr = findings.as_array().expect("findings is an array");

    let crash = arr.iter().find(|f| f["kind"] == "crash");
    assert!(crash.is_some(), "expected a crash finding, got: {report}");
    let crash = crash.unwrap();
    assert_eq!(crash["tool"], "fragile");
    assert_eq!(crash["severity"], "high");

    let _ = std::fs::remove_file(&json);
}

#[test]
fn only_flag_filters_to_one_tool() {
    let status = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--tool",
            "echo",
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-fuzz");
    // echo is robust → no high findings → exit 0.
    assert_eq!(status.code(), Some(0));
}
