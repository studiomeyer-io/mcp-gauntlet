//! Minimal JSON-RPC 2.0 + MCP wire types.
//!
//! Only the slice of the protocol the probing tools need is modelled: `initialize`,
//! `tools/list` and `tools/call`. Everything is deliberately lenient on deserialization
//! (unknown fields ignored, sensible defaults) so we stay compatible across MCP spec
//! revisions (2025-06-18 → 2025-11-25 → the 2026-07-28 RC).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC protocol version string.
pub const JSONRPC_VERSION: &str = "2.0";

/// Latest stable MCP protocol version this client advertises by default.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

/// A decoded JSON-RPC response (or notification echo) from the server.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    /// Request id this response correlates to. May be absent for server notifications.
    #[serde(default)]
    pub id: Option<Value>,
    /// Result payload on success.
    #[serde(default)]
    pub result: Option<Value>,
    /// Error object on failure.
    #[serde(default)]
    pub error: Option<RpcError>,
}

/// A JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    /// Numeric error code (e.g. `-32602` invalid params, `-32601` method not found).
    pub code: i64,
    /// Human-readable error message.
    pub message: String,
    /// Optional structured error data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// One tool exposed by an MCP server (`tools/list` entry).
#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    /// Unique tool name.
    pub name: String,
    /// Optional human/LLM-facing description.
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema for the tool's arguments.
    #[serde(rename = "inputSchema", default = "empty_object")]
    pub input_schema: Value,
    /// Optional JSON Schema for the tool's structured output.
    #[serde(rename = "outputSchema", default)]
    pub output_schema: Option<Value>,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// Result of a `tools/list` call.
#[derive(Debug, Clone, Deserialize)]
pub struct ListToolsResult {
    /// Tools advertised by the server.
    #[serde(default)]
    pub tools: Vec<Tool>,
}

/// Result of a `tools/call` call.
#[derive(Debug, Clone, Deserialize)]
pub struct CallToolResult {
    /// Content blocks returned by the tool.
    #[serde(default)]
    pub content: Vec<Value>,
    /// Whether the tool itself reported a (handled) error.
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

/// Result of the `initialize` handshake.
#[derive(Debug, Clone, Deserialize)]
pub struct InitializeResult {
    /// Protocol version the server negotiated.
    #[serde(rename = "protocolVersion", default)]
    pub protocol_version: String,
    /// Server capability advertisement.
    #[serde(default)]
    pub capabilities: Value,
    /// Server name/version metadata.
    #[serde(rename = "serverInfo", default)]
    pub server_info: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tools_list_result() {
        let raw = r#"{"tools":[{"name":"echo","description":"e","inputSchema":{"type":"object","properties":{"m":{"type":"string"}},"required":["m"]}}]}"#;
        let r: ListToolsResult = serde_json::from_str(raw).unwrap();
        assert_eq!(r.tools.len(), 1);
        assert_eq!(r.tools[0].name, "echo");
        assert_eq!(r.tools[0].input_schema["type"], "object");
    }

    #[test]
    fn tool_without_input_schema_defaults_to_empty_object() {
        let r: Tool = serde_json::from_str(r#"{"name":"x"}"#).unwrap();
        assert!(r.input_schema.is_object());
        assert!(r.description.is_none());
    }

    #[test]
    fn parses_call_result_and_error_flag() {
        let r: CallToolResult =
            serde_json::from_str(r#"{"content":[{"type":"text","text":"hi"}],"isError":true}"#)
                .unwrap();
        assert!(r.is_error);
        assert_eq!(r.content.len(), 1);
    }

    #[test]
    fn parses_rpc_error_response() {
        let r: JsonRpcResponse = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32602,"message":"bad"}}"#,
        )
        .unwrap();
        assert!(r.result.is_none());
        let e = r.error.unwrap();
        assert_eq!(e.code, -32602);
        assert_eq!(r.id.unwrap().as_u64(), Some(7));
    }

    #[test]
    fn ignores_unknown_fields() {
        let r: InitializeResult = serde_json::from_str(
            r#"{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s"},"_extra":42}"#,
        )
        .unwrap();
        assert_eq!(r.protocol_version, "2025-11-25");
    }
}
