//! # mcp-probe-core
//!
//! Shared building blocks for probing [Model Context Protocol](https://modelcontextprotocol.io)
//! servers. This crate is the foundation under the [`mcp-fuzz`] and [`mcp-storm`] CLIs but is
//! usable on its own as a small, concurrency-capable MCP client.
//!
//! It provides:
//!
//! * [`McpClient`] — an async JSON-RPC 2.0 client that speaks MCP over **stdio**
//!   (subprocess) or **Streamable HTTP**, with request multiplexing so many calls can be
//!   in flight over a single stdio connection.
//! * [`schema`] — schema-driven value generation: produce a *valid* value for a tool's
//!   `inputSchema`, or a battery of hostile/boundary [`schema::Mutation`]s derived from it.
//! * [`mock`] — a tiny in-process MCP server used for hermetic self-tests and demos.
//!
//! [`mcp-fuzz`]: https://crates.io/crates/mcp-fuzz
//! [`mcp-storm`]: https://crates.io/crates/mcp-storm
#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

pub mod client;
pub mod error;
pub mod mock;
pub mod protocol;
pub mod schema;

pub use client::McpClient;
pub use error::Error;
pub use protocol::{
    CallToolResult, InitializeResult, RpcError, Tool, JSONRPC_VERSION, LATEST_PROTOCOL_VERSION,
};
