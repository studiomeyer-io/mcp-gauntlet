//! Findings produced by a fuzzing run.

use mcp_gauntlet_core::protocol::RpcError;
use mcp_gauntlet_core::schema::Mutation;
use serde::Serialize;

/// Severity of a finding. Ordered `Info < Low < Medium < High` for `--fail-on` gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational, no action required.
    Info,
    /// Low: minor robustness gap.
    Low,
    /// Medium: server-internal error surfaced.
    Medium,
    /// High: crash or hang — the server stopped responding.
    High,
}

impl Severity {
    /// Uppercase label for reports.
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "INFO",
            Severity::Low => "LOW",
            Severity::Medium => "MEDIUM",
            Severity::High => "HIGH",
        }
    }

    /// SARIF `level` mapping.
    pub fn sarif_level(self) -> &'static str {
        match self {
            Severity::High => "error",
            Severity::Medium => "warning",
            Severity::Low | Severity::Info => "note",
        }
    }
}

/// A single problem discovered while fuzzing a tool.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// How serious it is.
    pub severity: Severity,
    /// Stable machine-readable kind, used as the SARIF `ruleId`.
    pub kind: &'static str,
    /// Tool that was being fuzzed.
    pub tool: String,
    /// Mutation category that triggered it.
    pub category: &'static str,
    /// Human-readable description of the input.
    pub description: String,
    /// The exact arguments sent, so the finding is reproducible.
    pub arguments: serde_json::Value,
    /// Extra detail (stderr tail, error message, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Finding {
    /// The server process died while handling this input.
    pub fn crash(tool: &str, m: &Mutation, stderr: Option<String>) -> Self {
        Finding {
            severity: Severity::High,
            kind: "crash",
            tool: tool.to_string(),
            category: m.category.as_str(),
            description: format!("server crashed on {}", m.description),
            arguments: m.arguments.clone(),
            detail: stderr.map(|s| format!("stderr tail:\n{s}")),
        }
    }

    /// The server did not respond within the timeout.
    pub fn hang(tool: &str, m: &Mutation, timeout_ms: u64) -> Self {
        Finding {
            severity: Severity::High,
            kind: "hang",
            tool: tool.to_string(),
            category: m.category.as_str(),
            description: format!("server hung (>{timeout_ms}ms) on {}", m.description),
            arguments: m.arguments.clone(),
            detail: None,
        }
    }

    /// The server returned a JSON-RPC internal error (`-32603`).
    pub fn internal_error(tool: &str, m: &Mutation, err: &RpcError) -> Self {
        Finding {
            severity: Severity::Medium,
            kind: "internal-error",
            tool: tool.to_string(),
            category: m.category.as_str(),
            description: format!("internal error on {}", m.description),
            arguments: m.arguments.clone(),
            detail: Some(format!("code {}: {}", err.code, err.message)),
        }
    }

    /// The server accepted input that clearly violates its own schema.
    pub fn accepted_invalid(tool: &str, m: &Mutation) -> Self {
        Finding {
            severity: Severity::Low,
            kind: "accepted-invalid",
            tool: tool.to_string(),
            category: m.category.as_str(),
            description: format!(
                "schema-invalid input accepted without error: {}",
                m.description
            ),
            arguments: m.arguments.clone(),
            detail: Some(
                "the tool returned a successful result for input that violates its inputSchema; \
                 add validation to reject it"
                    .to_string(),
            ),
        }
    }

    /// A transport-level error occurred.
    pub fn transport(tool: &str, m: &Mutation, detail: String) -> Self {
        Finding {
            severity: Severity::Low,
            kind: "transport-error",
            tool: tool.to_string(),
            category: m.category.as_str(),
            description: format!("transport error on {}", m.description),
            arguments: m.arguments.clone(),
            detail: Some(detail),
        }
    }

    /// The server did not come back after a crash/hang, so fuzzing stopped. One
    /// such finding replaces the storm of mis-attributed false crashes a dead
    /// server would otherwise produce.
    pub fn not_recovered(tool: &str) -> Self {
        Finding {
            severity: Severity::High,
            kind: "server-unrecovered",
            tool: tool.to_string(),
            category: "lifecycle",
            description: format!(
                "server did not recover after a crash/hang while fuzzing '{tool}'; \
                 stopped to avoid blaming later payloads"
            ),
            arguments: serde_json::Value::Null,
            detail: None,
        }
    }
}
