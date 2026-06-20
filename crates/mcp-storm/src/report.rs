//! Stats computation and terminal report for a load-test run.

use std::time::Duration;

use hdrhistogram::Histogram;
use mcp_gauntlet_core::protocol::InitializeResult;
use serde::Serialize;

/// Per-tool latency + throughput statistics (all latencies in milliseconds).
#[derive(Debug, Clone, Serialize)]
pub struct ToolStats {
    /// Tool name.
    pub tool: String,
    /// Total calls made (successes + errors).
    pub count: u64,
    /// Number of transport/timeout/RPC errors.
    pub errors: u64,
    /// Error rate as a percentage (0–100).
    pub error_rate_pct: f64,
    /// Calls per second over the measured window (all calls, errors included).
    pub throughput_rps: f64,
    /// Median latency (ms).
    pub p50_ms: f64,
    /// 95th percentile latency (ms).
    pub p95_ms: f64,
    /// 99th percentile latency (ms).
    pub p99_ms: f64,
    /// Maximum observed latency (ms).
    pub max_ms: f64,
    /// Mean latency (ms).
    pub mean_ms: f64,
}

impl ToolStats {
    /// Derive stats from a worker's merged histogram + counters.
    pub fn from_acc(
        tool: String,
        hist: &Histogram<u64>,
        count: u64,
        errors: u64,
        elapsed: Duration,
    ) -> Self {
        let ms = |micros: f64| micros / 1000.0;
        let secs = elapsed.as_secs_f64().max(1e-9);
        let (p50, p95, p99, max, mean) = if hist.is_empty() {
            (0.0, 0.0, 0.0, 0.0, 0.0)
        } else {
            (
                ms(hist.value_at_quantile(0.50) as f64),
                ms(hist.value_at_quantile(0.95) as f64),
                ms(hist.value_at_quantile(0.99) as f64),
                ms(hist.max() as f64),
                ms(hist.mean()),
            )
        };
        ToolStats {
            tool,
            count,
            errors,
            error_rate_pct: if count == 0 {
                0.0
            } else {
                errors as f64 / count as f64 * 100.0
            },
            throughput_rps: count as f64 / secs,
            p50_ms: p50,
            p95_ms: p95,
            p99_ms: p99,
            max_ms: max,
            mean_ms: mean,
        }
    }
}

/// Print the human-readable summary to stdout.
pub fn print_summary(
    init: &InitializeResult,
    stats: &[ToolStats],
    elapsed: Duration,
    concurrency: usize,
) {
    let name = init
        .server_info
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let total: u64 = stats.iter().map(|s| s.count).sum();
    let errs: u64 = stats.iter().map(|s| s.errors).sum();

    println!("\nmcp-storm report");
    println!("────────────────");
    println!("server       {name} (protocol {})", init.protocol_version);
    println!("workers      {concurrency}");
    println!("duration     {:.2}s", elapsed.as_secs_f64());
    println!(
        "requests     {total} ({errs} errors, {:.2}% error rate)",
        if total == 0 {
            0.0
        } else {
            errs as f64 / total as f64 * 100.0
        }
    );
    println!(
        "throughput   {:.0} req/s",
        total as f64 / elapsed.as_secs_f64().max(1e-9)
    );

    println!(
        "\n{:<20} {:>8} {:>7} {:>9} {:>9} {:>9} {:>9} {:>10}",
        "tool", "calls", "err%", "p50ms", "p95ms", "p99ms", "maxms", "req/s"
    );
    println!("{}", "─".repeat(86));
    for s in stats {
        println!(
            "{:<20} {:>8} {:>6.2}% {:>9.2} {:>9.2} {:>9.2} {:>9.2} {:>10.0}",
            truncate(&s.tool, 20),
            s.count,
            s.error_rate_pct,
            s.p50_ms,
            s.p95_ms,
            s.p99_ms,
            s.max_ms,
            s.throughput_rps,
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    // Count by chars, never byte-slice — tool names are server-controlled and may
    // contain multi-byte UTF-8 that would panic a byte slice off a char boundary.
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let kept: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{kept}…")
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_is_char_safe_on_multibyte() {
        // A server-controlled tool name longer than `max` with a multi-byte char
        // near the cut must truncate by chars, never panic on a byte boundary.
        let name = "abcdefghijklmnopqré-stuvwxyz0123";
        let out = truncate(name, 20);
        assert!(out.chars().count() <= 20);
        assert!(out.ends_with('…'));
        assert_eq!(truncate("short", 20), "short");
    }
}
