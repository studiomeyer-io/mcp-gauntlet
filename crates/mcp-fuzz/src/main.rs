//! `mcp-fuzz` — a schema-aware fuzzer for Model Context Protocol servers.
//!
//! ```text
//! mcp-fuzz run --stdio -- node my-server.js
//! mcp-fuzz run --http https://example.com/mcp --sarif findings.sarif
//! ```
#![forbid(unsafe_code)]

mod engine;
mod finding;
mod report;
mod sarif;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "mcp-fuzz",
    version,
    about = "Schema-aware fuzzer for MCP servers — finds crashes, hangs and validation gaps.",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fuzz an MCP server over stdio or HTTP.
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
