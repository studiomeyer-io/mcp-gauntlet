//! A tiny in-process MCP server over stdio.
//!
//! Used for hermetic self-tests (`mcp-fuzz mock`, `mcp-storm mock`) and as a live demo
//! target. It implements `initialize`, `tools/list` and `tools/call` for four tools:
//!
//! * `echo` — returns the message (robust).
//! * `slow` — sleeps for `ms` milliseconds (drives latency in load tests).
//! * `divide` — `a / b`, cleanly reports division-by-zero as an `isError` result.
//! * `fragile` — **intentionally crashes** (exits 101) on hostile input (very long strings
//!   or embedded NUL bytes), so the fuzzer has a deterministic finding to catch.

use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// Run the mock server, reading newline-delimited JSON-RPC from stdin and replying on
/// stdout. Blocks until stdin closes, then exits the process.
pub fn run_stdio_mock() -> ! {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let reader = stdin.lock();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();

        match method {
            "initialize" => reply(
                &mut out,
                id,
                json!({
                    "protocolVersion": crate::protocol::LATEST_PROTOCOL_VERSION,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "mcp-probe-mock", "version": env!("CARGO_PKG_VERSION")}
                }),
            ),
            "notifications/initialized" => { /* notification: no reply */ }
            "ping" => reply(&mut out, id, json!({})),
            "tools/list" => reply(&mut out, id, json!({ "tools": tool_defs() })),
            "tools/call" => handle_call(&mut out, id, msg.get("params")),
            other => {
                if id.is_some() {
                    reply_err(&mut out, id, -32601, &format!("method not found: {other}"));
                }
            }
        }
    }
    std::process::exit(0);
}

fn tool_defs() -> Value {
    json!([
        {
            "name": "echo",
            "description": "Echo a message back.",
            "inputSchema": {
                "type": "object",
                "properties": {"message": {"type": "string"}},
                "required": ["message"]
            }
        },
        {
            "name": "slow",
            "description": "Sleep for `ms` milliseconds, then return.",
            "inputSchema": {
                "type": "object",
                "properties": {"ms": {"type": "integer", "minimum": 0, "maximum": 60000}},
                "required": ["ms"]
            }
        },
        {
            "name": "divide",
            "description": "Divide a by b.",
            "inputSchema": {
                "type": "object",
                "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
                "required": ["a", "b"]
            }
        },
        {
            "name": "fragile",
            "description": "Self-test fixture: intentionally crashes on hostile input.",
            "inputSchema": {
                "type": "object",
                "properties": {"input": {"type": "string"}},
                "required": ["input"]
            }
        }
    ])
}

fn handle_call<W: Write>(out: &mut W, id: Option<Value>, params: Option<&Value>) {
    let params = params.cloned().unwrap_or_else(|| json!({}));
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "echo" => match args.get("message").and_then(Value::as_str) {
            Some(m) => reply(out, id, text_result(m, false)),
            None => reply_err(out, id, -32602, "missing required 'message' (string)"),
        },
        "slow" => {
            let ms = args
                .get("ms")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(60_000);
            std::thread::sleep(std::time::Duration::from_millis(ms));
            reply(out, id, text_result(&format!("slept {ms}ms"), false));
        }
        "divide" => {
            let a = args.get("a").and_then(Value::as_f64);
            let b = args.get("b").and_then(Value::as_f64);
            match (a, b) {
                (Some(_), Some(0.0)) => reply(out, id, text_result("division by zero", true)),
                (Some(a), Some(b)) => reply(out, id, text_result(&format!("{}", a / b), false)),
                _ => reply_err(out, id, -32602, "missing/invalid 'a' or 'b' (number)"),
            }
        }
        "fragile" => match args.get("input").and_then(Value::as_str) {
            Some(s) if s.len() > 5000 || s.contains('\u{0}') => {
                eprintln!(
                    "mcp-probe-mock: FATAL: hostile input to 'fragile' (len={}, has_nul={})",
                    s.len(),
                    s.contains('\u{0}')
                );
                std::process::exit(101);
            }
            Some(s) => reply(
                out,
                id,
                text_result(&format!("ok: {} chars", s.chars().count()), false),
            ),
            None => reply_err(out, id, -32602, "missing required 'input' (string)"),
        },
        other => reply_err(out, id, -32602, &format!("unknown tool: {other}")),
    }
}

fn text_result(text: &str, is_error: bool) -> Value {
    json!({ "content": [{"type": "text", "text": text}], "isError": is_error })
}

fn reply<W: Write>(out: &mut W, id: Option<Value>, result: Value) {
    let _ = writeln!(
        out,
        "{}",
        json!({"jsonrpc": "2.0", "id": id, "result": result})
    );
    let _ = out.flush();
}

fn reply_err<W: Write>(out: &mut W, id: Option<Value>, code: i64, message: &str) {
    let _ = writeln!(
        out,
        "{}",
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
    );
    let _ = out.flush();
}
