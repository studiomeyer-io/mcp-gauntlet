//! Terminal report rendering for a fuzzing run.

use crate::finding::{Finding, Severity};
use mcp_gauntlet_core::protocol::{InitializeResult, Tool};

/// Print the human-readable summary to stdout.
pub fn print_summary(init: &InitializeResult, tools: &[Tool], tested: usize, findings: &[Finding]) {
    let name = init
        .server_info
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    println!("\nmcp-fuzz report");
    println!("───────────────");
    println!("server         {name} (protocol {})", init.protocol_version);
    println!("tools          {}", tools.len());
    println!("payloads sent  {tested}");

    let (mut high, mut medium, mut low, mut info) = (0u32, 0u32, 0u32, 0u32);
    for f in findings {
        match f.severity {
            Severity::High => high += 1,
            Severity::Medium => medium += 1,
            Severity::Low => low += 1,
            Severity::Info => info += 1,
        }
    }
    println!(
        "findings       {} total — {high} high, {medium} medium, {low} low, {info} info",
        findings.len()
    );

    if findings.is_empty() {
        println!("\nNo crashes, hangs or internal errors. Server held up.");
        return;
    }

    println!("\nFindings (worst first):");
    let mut sorted: Vec<&Finding> = findings.iter().collect();
    sorted.sort_by(|a, b| b.severity.cmp(&a.severity));
    for f in sorted {
        println!(
            "  [{:<6}] {:<16} {}::{}",
            f.severity.label(),
            f.kind,
            f.tool,
            f.description
        );
        if let Some(d) = &f.detail {
            for line in d.lines().take(4) {
                println!("           {line}");
            }
        }
    }
}
