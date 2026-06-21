//! Hermetic end-to-end test: load test the bundled mock server (via the hidden `mock`
//! subcommand) and assert stats are produced and the CI gate behaves.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mcp-storm")
}

#[test]
fn load_test_against_mock_produces_stats() {
    let dir = std::env::temp_dir();
    let json = dir.join(format!("mcp-storm-selftest-{}.json", std::process::id()));

    let status = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--tool",
            "echo",
            "--concurrency",
            "4",
            "--requests",
            "80",
            "--json",
            json.to_str().unwrap(),
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-storm");
    assert_eq!(status.code(), Some(0), "clean load test should exit 0");

    let report = std::fs::read_to_string(&json).expect("json written");
    let stats: serde_json::Value = serde_json::from_str(&report).unwrap();
    let arr = stats.as_array().expect("array");
    let echo = arr
        .iter()
        .find(|s| s["tool"] == "echo")
        .expect("echo stats");
    assert!(
        echo["count"].as_u64().unwrap() >= 1,
        "should have made calls"
    );
    assert_eq!(echo["errors"].as_u64().unwrap(), 0, "echo should not error");

    let _ = std::fs::remove_file(&json);
}

#[test]
fn impossible_p95_threshold_fails_the_gate() {
    let status = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--tool",
            "echo",
            "--concurrency",
            "2",
            "--requests",
            "40",
            "--threshold-p95-ms",
            "0",
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-storm");
    // p95 can't be <= 0ms → gate fails → exit 1.
    assert_eq!(
        status.code(),
        Some(1),
        "impossible threshold should fail gate"
    );
}

#[test]
fn generous_thresholds_pass_the_gate() {
    // Regression guard against an "always exit 0" *or* "always fail" gate: with a roomy
    // p95 budget and error-rate ceiling, a healthy echo run must exit 0 *because the
    // gate evaluated and passed*, not because gating is a no-op.
    let status = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--tool",
            "echo",
            "--concurrency",
            "2",
            "--requests",
            "40",
            "--threshold-p95-ms",
            "5000",
            "--max-error-rate",
            "1.0",
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-storm");
    assert_eq!(
        status.code(),
        Some(0),
        "healthy run under generous thresholds should pass the gate"
    );
}

#[test]
fn error_rate_gate_fails_when_calls_time_out() {
    // Drive the `slow` tool (the mock sleeps for its `ms` arg, generated near its
    // maximum) with a 1ms per-call timeout so every call times out → 100% error rate →
    // the --max-error-rate gate must fail (exit 1). Proves errors actually feed the gate.
    let status = Command::new(bin())
        .args([
            "run",
            "--stdio",
            "--quiet",
            "--tool",
            "slow",
            "--concurrency",
            "2",
            "--requests",
            "8",
            "--timeout-ms",
            "1",
            "--max-error-rate",
            "50.0",
            "--",
            bin(),
            "mock",
        ])
        .status()
        .expect("failed to launch mcp-storm");
    assert_eq!(
        status.code(),
        Some(1),
        "timed-out calls should push the error rate over the gate"
    );
}
