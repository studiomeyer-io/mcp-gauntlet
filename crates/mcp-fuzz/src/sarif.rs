//! SARIF 2.1.0 serialization so findings drop straight into GitHub code scanning.

use crate::finding::Finding;
use serde_json::{json, Value};
use std::hash::{Hash, Hasher};

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
                "locations": [{
                    "logicalLocations": [{
                        "name": f.tool,
                        "fullyQualifiedName": format!("tool/{}", f.tool),
                        "kind": "function"
                    }]
                }],
                "partialFingerprints": { "mcpProbe/v1": fingerprint(f) },
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
                    "informationUri": "https://github.com/studiomeyer-io/mcp-gauntlet",
                    "rules": rules(),
                }
            },
            "results": results,
        }]
    });

    serde_json::to_vec_pretty(&doc).unwrap_or_else(|_| b"{}".to_vec())
}

/// Stable per-finding fingerprint for SARIF dedup across runs: tool + kind +
/// category + the exact arguments. Uses the std fixed-seed hasher (deterministic
/// within a toolchain), which is what GitHub code scanning needs for dedup.
fn fingerprint(f: &Finding) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.tool.hash(&mut h);
    f.kind.hash(&mut h);
    f.category.hash(&mut h);
    f.arguments.to_string().hash(&mut h);
    format!("{:016x}", h.finish())
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
        rule(
            "server-unrecovered",
            "Server did not recover",
            "The server did not come back after a crash/hang, so fuzzing stopped."
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

#[cfg(test)]
mod tests {
    use super::{fingerprint, to_sarif};
    use crate::finding::{Finding, Severity};
    use mcp_gauntlet_core::schema::{Mutation, MutationCategory};
    use serde_json::Value;

    fn sample_mutation() -> Mutation {
        Mutation {
            category: MutationCategory::TypeConfusion,
            description: "field 'q' wrong type (declared string)".to_string(),
            arguments: serde_json::json!({"q": 12345}),
            clear_violation: true,
        }
    }

    fn findings() -> Vec<Finding> {
        let m = sample_mutation();
        vec![
            Finding::crash("search", &m, Some("panic: boom".to_string())),
            Finding::hang("search", &m, 5000),
            Finding::internal_error(
                "search",
                &m,
                &mcp_gauntlet_core::protocol::RpcError {
                    code: -32603,
                    message: "kaboom".to_string(),
                    data: None,
                },
            ),
            Finding::accepted_invalid("search", &m),
            Finding::not_recovered("search"),
        ]
    }

    #[test]
    fn sarif_document_has_required_top_level_shape() {
        let doc: Value = serde_json::from_slice(&to_sarif(&findings())).unwrap();
        assert_eq!(doc["version"], "2.1.0");
        assert!(doc["$schema"].as_str().unwrap().contains("sarif-2.1.0"));
        let runs = doc["runs"].as_array().expect("runs array");
        assert_eq!(runs.len(), 1);
        let driver = &runs[0]["tool"]["driver"];
        assert_eq!(driver["name"], "mcp-fuzz");
        assert!(driver["version"].is_string());
        assert!(driver["rules"].as_array().unwrap().len() >= 6);
    }

    #[test]
    fn every_result_is_well_formed_and_uses_a_valid_level() {
        let doc: Value = serde_json::from_slice(&to_sarif(&findings())).unwrap();
        let results = doc["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 5);
        for r in results {
            // ruleId + message.text are mandatory in SARIF.
            assert!(r["ruleId"].is_string(), "missing ruleId: {r}");
            assert!(
                r["message"]["text"].is_string(),
                "missing message.text: {r}"
            );
            // SARIF `level` must be one of the enumerated values.
            let level = r["level"].as_str().expect("level string");
            assert!(
                matches!(level, "error" | "warning" | "note" | "none"),
                "invalid SARIF level: {level}"
            );
            // a logical location keeps the finding tied to its tool.
            assert!(r["locations"][0]["logicalLocations"][0]["name"].is_string());
            // partial fingerprints power GitHub's cross-run dedup.
            assert!(r["partialFingerprints"]["mcpProbe/v1"].is_string());
        }
    }

    #[test]
    fn severity_maps_to_the_expected_sarif_level() {
        assert_eq!(Severity::High.sarif_level(), "error");
        assert_eq!(Severity::Medium.sarif_level(), "warning");
        assert_eq!(Severity::Low.sarif_level(), "note");
        assert_eq!(Severity::Info.sarif_level(), "note");
    }

    #[test]
    fn fingerprint_is_stable_and_distinguishes_findings() {
        let m = sample_mutation();
        let a = Finding::crash("search", &m, None);
        let b = Finding::crash("search", &m, None);
        // Same tool+kind+category+arguments → identical fingerprint (cross-run dedup).
        assert_eq!(fingerprint(&a), fingerprint(&b));
        // Different tool → different fingerprint.
        let c = Finding::crash("other", &m, None);
        assert_ne!(fingerprint(&a), fingerprint(&c));
        // 16 lowercase hex chars.
        let fp = fingerprint(&a);
        assert_eq!(fp.len(), 16);
        assert!(fp.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn empty_findings_still_produce_a_valid_sarif_skeleton() {
        let doc: Value = serde_json::from_slice(&to_sarif(&[])).unwrap();
        assert_eq!(doc["version"], "2.1.0");
        assert_eq!(doc["runs"][0]["results"].as_array().unwrap().len(), 0);
    }
}
