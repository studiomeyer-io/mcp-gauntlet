//! Async MCP client over stdio (subprocess) or Streamable HTTP.
//!
//! The client is [`Clone`] (cheap `Arc` handle) and supports **concurrent** in-flight
//! requests: over a single stdio pipe, responses are demultiplexed back to the right caller
//! by JSON-RPC id. This is what lets `mcp-storm` drive N concurrent workers against one
//! subprocess connection.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

use crate::error::Error;
use crate::protocol::{
    CallToolResult, InitializeResult, JsonRpcResponse, ListToolsResult, RpcError, Tool,
    JSONRPC_VERSION, LATEST_PROTOCOL_VERSION,
};

/// How many trailing stderr lines to retain for crash diagnostics.
const STDERR_TAIL: usize = 60;

/// Hard cap on a single JSON-RPC line. A server that never sends a newline can't
/// make us buffer unboundedly (DoS); 16 MiB is far above any real MCP message.
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// An MCP client handle. Clone freely; clones share the same connection.
#[derive(Clone, Debug)]
pub struct McpClient {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    transport: Transport,
    next_id: AtomicU64,
    default_timeout: Duration,
}

#[derive(Debug)]
enum Transport {
    Stdio(Arc<StdioInner>),
    #[cfg(feature = "http")]
    Http(HttpInner),
}

type Pending = Mutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>;

#[derive(Debug)]
struct StdioInner {
    writer_tx: mpsc::UnboundedSender<Vec<u8>>,
    pending: Pending,
    alive: AtomicBool,
    stderr_tail: Arc<Mutex<Vec<String>>>,
    child: Mutex<Option<tokio::process::Child>>,
}

impl StdioInner {
    fn stderr_snapshot(&self) -> Option<String> {
        let t = self.stderr_tail.lock().unwrap();
        if t.is_empty() {
            None
        } else {
            Some(t.join("\n"))
        }
    }
}

impl Drop for StdioInner {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.start_kill();
            }
        }
    }
}

#[cfg(feature = "http")]
#[derive(Debug)]
struct HttpInner {
    client: reqwest::Client,
    url: String,
    session: Mutex<Option<String>>,
    protocol_version: String,
}

impl McpClient {
    /// Launch `program args...` as a subprocess and speak MCP over its stdio.
    pub async fn connect_stdio(
        program: &str,
        args: &[String],
        default_timeout: Duration,
    ) -> Result<McpClient, Error> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Protocol("child stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Protocol("child stdout unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::Protocol("child stderr unavailable".into()))?;

        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let stderr_tail = Arc::new(Mutex::new(Vec::<String>::new()));
        let state = Arc::new(StdioInner {
            writer_tx,
            pending: Mutex::new(HashMap::new()),
            alive: AtomicBool::new(true),
            stderr_tail: stderr_tail.clone(),
            child: Mutex::new(Some(child)),
        });

        // Writer task: drains the outbound queue into the child's stdin.
        tokio::spawn(async move {
            while let Some(buf) = writer_rx.recv().await {
                if stdin.write_all(&buf).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        // Reader task: demultiplexes responses by id, with a hard per-line cap so a
        // hostile server that never sends a newline can't drive us to OOM. On
        // EOF/overflow the connection is marked dead and pending callers are failed.
        {
            let state = state.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                let mut line: Vec<u8> = Vec::with_capacity(8192);
                let mut chunk = [0u8; 8192];
                'read: loop {
                    let n = match reader.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let mut start = 0;
                    for i in 0..n {
                        if chunk[i] == b'\n' {
                            line.extend_from_slice(&chunk[start..i]);
                            dispatch_line(&line, &state);
                            line.clear();
                            start = i + 1;
                        }
                    }
                    line.extend_from_slice(&chunk[start..n]);
                    if line.len() > MAX_LINE_BYTES {
                        break 'read; // refuse to buffer an unbounded line
                    }
                }
                // EOF / read error / overflow → connection is dead.
                state.alive.store(false, Ordering::SeqCst);
                state.pending.lock().unwrap().clear(); // dropping senders surfaces ConnectionClosed
            });
        }

        // Stderr task: keep a rolling tail for crash diagnostics.
        {
            let tail = stderr_tail.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut t = tail.lock().unwrap();
                    t.push(line);
                    let len = t.len();
                    if len > STDERR_TAIL {
                        t.drain(0..len - STDERR_TAIL);
                    }
                }
            });
        }

        Ok(McpClient {
            inner: Arc::new(Inner {
                transport: Transport::Stdio(state),
                next_id: AtomicU64::new(1),
                default_timeout,
            }),
        })
    }

    /// Connect to a Streamable HTTP MCP endpoint.
    #[cfg(feature = "http")]
    pub async fn connect_http(url: &str, default_timeout: Duration) -> Result<McpClient, Error> {
        let client = reqwest::Client::builder()
            .timeout(default_timeout + Duration::from_secs(5))
            .user_agent(concat!("mcp-probe/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(McpClient {
            inner: Arc::new(Inner {
                transport: Transport::Http(HttpInner {
                    client,
                    url: url.to_string(),
                    session: Mutex::new(None),
                    protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                }),
                next_id: AtomicU64::new(1),
                default_timeout,
            }),
        })
    }

    /// Whether the underlying connection is still believed to be alive.
    pub fn is_alive(&self) -> bool {
        match &self.inner.transport {
            Transport::Stdio(s) => s.alive.load(Ordering::SeqCst),
            #[cfg(feature = "http")]
            Transport::Http(_) => true,
        }
    }

    /// Last captured stderr from a stdio server, if any (useful in crash reports).
    pub fn stderr_tail(&self) -> Option<String> {
        match &self.inner.transport {
            Transport::Stdio(s) => s.stderr_snapshot(),
            #[cfg(feature = "http")]
            Transport::Http(_) => None,
        }
    }

    /// Send a request using the default timeout.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        self.request_timeout(method, params, self.inner.default_timeout)
            .await
    }

    /// Send a request with an explicit per-call timeout.
    pub async fn request_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        match &self.inner.transport {
            Transport::Stdio(s) => self.stdio_request(s, method, params, timeout).await,
            #[cfg(feature = "http")]
            Transport::Http(h) => {
                let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
                let body =
                    json!({"jsonrpc":JSONRPC_VERSION,"id":id,"method":method,"params":params});
                let fut = self.http_post(h, &body);
                let text = tokio::time::timeout(timeout, fut)
                    .await
                    .map_err(|_| Error::Timeout)??;
                parse_rpc_payload(&text, id)
            }
        }
    }

    /// Send a notification (no response expected).
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), Error> {
        match &self.inner.transport {
            Transport::Stdio(s) => {
                if !s.alive.load(Ordering::SeqCst) {
                    return Err(Error::ConnectionClosed {
                        stderr: s.stderr_snapshot(),
                    });
                }
                let req = json!({"jsonrpc":JSONRPC_VERSION,"method":method,"params":params});
                let mut line = serde_json::to_vec(&req)?;
                line.push(b'\n');
                s.writer_tx
                    .send(line)
                    .map_err(|_| Error::ConnectionClosed {
                        stderr: s.stderr_snapshot(),
                    })?;
                Ok(())
            }
            #[cfg(feature = "http")]
            Transport::Http(h) => {
                let body = json!({"jsonrpc":JSONRPC_VERSION,"method":method,"params":params});
                let _ = self.http_post(h, &body).await?;
                Ok(())
            }
        }
    }

    async fn stdio_request(
        &self,
        s: &Arc<StdioInner>,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        if !s.alive.load(Ordering::SeqCst) {
            return Err(Error::ConnectionClosed {
                stderr: s.stderr_snapshot(),
            });
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        s.pending.lock().unwrap().insert(id, tx);

        let req = json!({"jsonrpc":JSONRPC_VERSION,"id":id,"method":method,"params":params});
        let mut line = serde_json::to_vec(&req)?;
        line.push(b'\n');
        if s.writer_tx.send(line).is_err() {
            s.pending.lock().unwrap().remove(&id);
            return Err(Error::ConnectionClosed {
                stderr: s.stderr_snapshot(),
            });
        }

        match tokio::time::timeout(timeout, rx).await {
            Err(_) => {
                s.pending.lock().unwrap().remove(&id);
                Err(Error::Timeout)
            }
            Ok(Err(_)) => Err(Error::ConnectionClosed {
                stderr: s.stderr_snapshot(),
            }),
            Ok(Ok(Err(rpc))) => Err(Error::Rpc(rpc)),
            Ok(Ok(Ok(v))) => Ok(v),
        }
    }

    #[cfg(feature = "http")]
    async fn http_post(&self, h: &HttpInner, body: &Value) -> Result<String, Error> {
        let mut req = h
            .client
            .post(&h.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", &h.protocol_version)
            .json(body);
        let session = h.session.lock().unwrap().clone();
        if let Some(s) = session {
            req = req.header("mcp-session-id", s);
        }
        let resp = req.send().await.map_err(|e| Error::Http(e.to_string()))?;
        if let Some(sid) = resp.headers().get("mcp-session-id") {
            if let Ok(s) = sid.to_str() {
                *h.session.lock().unwrap() = Some(s.to_string());
            }
        }
        let status = resp.status();
        let text = resp.text().await.map_err(|e| Error::Http(e.to_string()))?;
        if !status.is_success() && text.trim().is_empty() {
            return Err(Error::Http(format!("HTTP {status}")));
        }
        Ok(text)
    }

    // --- High-level convenience over MCP semantics ---

    /// Perform the `initialize` handshake (then send `notifications/initialized`).
    pub async fn initialize(&self) -> Result<InitializeResult, Error> {
        let params = json!({
            "protocolVersion": LATEST_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "mcp-probe", "version": env!("CARGO_PKG_VERSION")}
        });
        let res = self.request("initialize", params).await?;
        let init: InitializeResult = serde_json::from_value(res)?;
        let _ = self.notify("notifications/initialized", json!({})).await;
        Ok(init)
    }

    /// List the server's tools.
    pub async fn list_tools(&self) -> Result<Vec<Tool>, Error> {
        let res = self.request("tools/list", json!({})).await?;
        let lt: ListToolsResult = serde_json::from_value(res)?;
        Ok(lt.tools)
    }

    /// Call a tool with the default timeout.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<CallToolResult, Error> {
        self.call_tool_timeout(name, arguments, self.inner.default_timeout)
            .await
    }

    /// Call a tool with an explicit timeout.
    pub async fn call_tool_timeout(
        &self,
        name: &str,
        arguments: Value,
        timeout: Duration,
    ) -> Result<CallToolResult, Error> {
        let res = self
            .request_timeout(
                "tools/call",
                json!({"name": name, "arguments": arguments}),
                timeout,
            )
            .await?;
        Ok(serde_json::from_value(res)?)
    }
}

fn value_as_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Parse one newline-delimited JSON-RPC line and, if it correlates to a pending
/// request id, deliver the result/error to the waiting caller.
fn dispatch_line(line: &[u8], state: &StdioInner) {
    if line.iter().all(u8::is_ascii_whitespace) {
        return;
    }
    if let Ok(resp) = serde_json::from_slice::<JsonRpcResponse>(line) {
        if let Some(id) = resp.id.as_ref().and_then(value_as_u64) {
            if let Some(tx) = state.pending.lock().unwrap().remove(&id) {
                let r = match resp.error {
                    Some(e) => Err(e),
                    None => Ok(resp.result.unwrap_or(Value::Null)),
                };
                let _ = tx.send(r);
            }
        }
        // Server-initiated requests/notifications are ignored by the probe.
    }
}

/// Parse a JSON-RPC payload that may arrive as a single JSON body or an SSE stream.
#[cfg(feature = "http")]
fn parse_rpc_payload(text: &str, id: u64) -> Result<Value, Error> {
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    // Server-Sent Events framing: pick the data line that matches our id.
    if text.contains("data:") {
        for line in text.lines() {
            let line = line.trim_start();
            if let Some(rest) = line.strip_prefix("data:") {
                let rest = rest.trim();
                if rest.is_empty() || rest == "[DONE]" {
                    continue;
                }
                if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(rest) {
                    let matches = resp.id.as_ref().and_then(value_as_u64) == Some(id);
                    if matches {
                        return finalize_response(resp);
                    }
                }
            }
        }
        return Err(Error::Protocol(
            "no JSON-RPC frame matching the request id in SSE response".into(),
        ));
    }
    let resp: JsonRpcResponse = serde_json::from_str(text)?;
    finalize_response(resp)
}

#[cfg(feature = "http")]
fn finalize_response(resp: JsonRpcResponse) -> Result<Value, Error> {
    match resp.error {
        Some(e) => Err(Error::Rpc(e)),
        None => Ok(resp.result.unwrap_or(Value::Null)),
    }
}

#[cfg(all(test, feature = "http"))]
mod http_tests {
    use super::*;

    #[test]
    fn parses_single_json_body() {
        let v = parse_rpc_payload(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#, 1).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn parses_sse_framed_body() {
        let sse = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"v\":9}}\n\n";
        let v = parse_rpc_payload(sse, 3).unwrap();
        assert_eq!(v["v"], 9);
    }

    #[test]
    fn surfaces_rpc_error() {
        let err = parse_rpc_payload(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}}"#,
            1,
        );
        assert!(matches!(err, Err(Error::Rpc(_))));
    }

    #[test]
    fn empty_body_is_null() {
        assert_eq!(parse_rpc_payload("", 1).unwrap(), Value::Null);
    }
}
