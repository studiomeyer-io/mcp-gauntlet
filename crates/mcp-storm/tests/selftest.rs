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
