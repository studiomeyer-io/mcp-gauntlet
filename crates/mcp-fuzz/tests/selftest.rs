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

#[test]
fn slow_tool_is_flagged_as_a_hang_under_a_tight_timeout() {
    // Boundary mutations push `slow`'s `ms` to extremes (i64::MAX → the mock clamps to a
    // 60s sleep). With a 200ms per-call timeout — comfortably above subprocess-spawn +
    // handshake latency, but far below that sleep — those calls must be classified as a
    // HIGH `hang` (not a crash, not silence) and the run exits non-zero on --fail-on high.
    let dir = std::env::temp_dir();
    let json = dir.join(format!("mcp-fuzz-hang-{}.json", std::process::id()));

    let status = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--tool",
            "slow",
            "--timeout-ms",
            "200",
            "--iterations",
            "12",
            "--json",
            json.to_str().unwrap(),
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-fuzz");
    assert_eq!(status.code(), Some(1), "a hang is a HIGH finding → exit 1");

    let report = std::fs::read_to_string(&json).expect("json written");
    let findings: serde_json::Value = serde_json::from_str(&report).unwrap();
    let arr = findings.as_array().expect("array");
    let hang = arr.iter().find(|f| f["kind"] == "hang");
    assert!(hang.is_some(), "expected a hang finding, got: {report}");
    assert_eq!(hang.unwrap()["severity"], "high");

    let _ = std::fs::remove_file(&json);
}

#[test]
fn sarif_output_is_written_and_well_formed() {
    let dir = std::env::temp_dir();
    let sarif = dir.join(format!("mcp-fuzz-{}.sarif", std::process::id()));

    // Fuzz the whole mock so the planted crash on `fragile` lands in the SARIF.
    let _ = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--seed",
            "7",
            "--sarif",
            sarif.to_str().unwrap(),
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-fuzz");

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&sarif).expect("sarif written")).unwrap();
    assert_eq!(doc["version"], "2.1.0");
    assert_eq!(doc["runs"][0]["tool"]["driver"]["name"], "mcp-fuzz");
    let results = doc["runs"][0]["results"].as_array().expect("results");
    assert!(
        results.iter().any(|r| r["ruleId"] == "crash"),
        "SARIF should carry the planted crash: {doc}"
    );
    for r in results {
        let level = r["level"].as_str().unwrap();
        assert!(matches!(level, "error" | "warning" | "note" | "none"));
    }

    let _ = std::fs::remove_file(&sarif);
}

#[test]
fn robust_mock_tools_yield_no_false_accepted_invalid() {
    // Regression for the union/required-field false-positive class: echo/divide/slow all
    // validate their required fields, so a full fuzz must not emit a single
    // `accepted-invalid` finding. (fragile crashes rather than mis-accepting.)
    let dir = std::env::temp_dir();
    let json = dir.join(format!("mcp-fuzz-fp-{}.json", std::process::id()));

    let _ = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--seed",
            "3",
            "--tool",
            "echo",
            "--tool",
            "divide",
            "--json",
            json.to_str().unwrap(),
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-fuzz");

    let report = std::fs::read_to_string(&json).expect("json written");
    let arr: serde_json::Value = serde_json::from_str(&report).unwrap();
    let accepted_invalid = arr
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "accepted-invalid")
        .count();
    assert_eq!(
        accepted_invalid, 0,
        "validating tools must not be flagged accepted-invalid: {report}"
    );

    let _ = std::fs::remove_file(&json);
}
