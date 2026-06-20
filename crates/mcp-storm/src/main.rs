//! `mcp-storm` — a load tester for Model Context Protocol servers.
//!
//! ```text
//! mcp-storm run --stdio --concurrency 16 --duration-secs 30 -- node my-server.js
//! mcp-storm run --http https://example.com/mcp --requests 1000 --threshold-p95-ms 250
//! ```
#![forbid(unsafe_code)]

mod engine;
mod report;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mcp-storm",
    version,
    about = "Load tester for MCP servers — concurrent tool calls, p50/p95/p99, CI gating.",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load test an MCP server over stdio or HTTP.
    Run(engine::RunArgs),
    /// (internal) Run the bundled mock MCP server used for self-tests and demos.
    #[command(hide = true)]
    Mock,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Mock => mcp_probe_core::mock::run_stdio_mock(),
        Command::Run(args) => {
            let code = engine::run(args).await?;
            std::process::exit(code);
        }
    }
}
