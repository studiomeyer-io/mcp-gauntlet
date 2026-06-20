//! SARIF 2.1.0 serialization so findings drop straight into GitHub code scanning.

use crate::finding::Finding;
use serde_json::{json, Value};

/// Render findings as a SARIF 2.1.0 document (pretty JSON bytes).
pub fn to_sarif(findings: &[Finding]) -> Vec<u8> {
    let results: Vec<Value> = findings
        .iter()
        .map(|f| {
            json!({
                "ruleId": f.kind,
                "level": f.severity.sarif_level(),
                "message": {
                    "text": format!("[{}] {} ({}): {}", f.tool, f.kind, f.category, f.description)
                },
                "properties": {
                    "tool": f.tool,
                    "severity": f.severity.label(),
                    "category": f.category,
                    "arguments": f.arguments,
                    "detail": f.detail,
                }
            })
        })
        .collect();

    let doc = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "mcp-fuzz",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/studiomeyer-io/mcp-probe",
                    "rules": rules(),
                }
            },
            "results": results,
        }]
    });

    serde_json::to_vec_pretty(&doc).unwrap_or_else(|_| b"{}".to_vec())
}

fn rules() -> Value {
    json!([
        rule(
            "crash",
            "Server crash",
            "The server process terminated while handling a tool call."
        ),
        rule(
            "hang",
            "Server hang",
            "The server did not respond within the timeout."
        ),
        rule(
            "internal-error",
            "Internal error",
            "The server returned JSON-RPC -32603 (internal error)."
        ),
        rule(
            "accepted-invalid",
            "Schema-invalid input accepted",
            "The server returned success for input violating its own inputSchema."
        ),
        rule(
            "transport-error",
            "Transport error",
            "A transport-level error occurred during a tool call."
        ),
    ])
}

fn rule(id: &str, name: &str, text: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "shortDescription": {"text": name},
        "fullDescription": {"text": text},
    })
}
