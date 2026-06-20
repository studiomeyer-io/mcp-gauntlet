//! The fuzzing engine: connect, discover tools, mutate, classify, report.

use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use mcp_gauntlet_core::schema;
use mcp_gauntlet_core::{Error, McpClient};
use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::finding::{Finding, Severity};
use crate::{report, sarif};

/// Arguments for `mcp-fuzz run`.
#[derive(Args, Debug)]
pub struct RunArgs {
    /// Launch the server as a subprocess and speak MCP over stdio (command follows `--`).
    #[arg(long)]
    pub stdio: bool,

    /// Connect to a Streamable HTTP MCP endpoint URL instead of a subprocess.
    #[arg(long, value_name = "URL")]
    pub http: Option<String>,

    /// For `--stdio`: the server command and its arguments (everything after `--`).
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "COMMAND"
    )]
    pub command: Vec<String>,

    /// Maximum number of mutated payloads per tool.
    #[arg(long, default_value_t = 80)]
    pub iterations: usize,

    /// Per-call timeout in milliseconds (exceeding it is flagged as a hang).
    #[arg(long, default_value_t = 5000)]
    pub timeout_ms: u64,

    /// Only fuzz this tool (repeatable). Default: all tools.
    #[arg(long = "tool", value_name = "NAME")]
    pub tools: Vec<String>,

    /// RNG seed for reproducible runs.
    #[arg(long, default_value_t = 0x00C0_FFEE)]
    pub seed: u64,

    /// Write a SARIF 2.1.0 report to this path (for GitHub code scanning).
    #[arg(long, value_name = "FILE")]
    pub sarif: Option<PathBuf>,

    /// Write a JSON report of all findings to this path.
    #[arg(long, value_name = "FILE")]
    pub json: Option<PathBuf>,

    /// Exit with code 1 when a finding of this severity or higher is present.
    #[arg(long, value_enum, default_value = "high")]
    pub fail_on: Severity,

    /// Suppress per-tool progress on stderr.
    #[arg(long)]
    pub quiet: bool,
}

enum Target {
    Stdio { program: String, args: Vec<String> },
    Http(String),
}

impl Target {
    fn from_args(a: &RunArgs) -> anyhow::Result<Self> {
        if let Some(url) = &a.http {
            return Ok(Target::Http(url.clone()));
        }
        if a.stdio || !a.command.is_empty() {
            let mut cmd = a.command.clone();
            if cmd.is_empty() {
                anyhow::bail!("--stdio requires a server command after `--`");
            }
            let program = cmd.remove(0);
            return Ok(Target::Stdio { program, args: cmd });
        }
        anyhow::bail!("specify either `--stdio -- <command>` or `--http <url>`");
    }

    async fn connect(&self, timeout: Duration) -> Result<McpClient, Error> {
        match self {
            Target::Stdio { program, args } => {
                McpClient::connect_stdio(program, args, timeout).await
            }
            #[cfg(feature = "http")]
            Target::Http(url) => McpClient::connect_http(url, timeout).await,
            #[cfg(not(feature = "http"))]
            Target::Http(url) => Err(Error::Protocol(format!(
                "HTTP transport not compiled in (enable the `http` feature); requested {url}"
            ))),
        }
    }
}

/// Run a fuzzing session. Returns the process exit code.
pub async fn run(args: RunArgs) -> anyhow::Result<i32> {
    let target = Target::from_args(&args)?;
    let timeout = Duration::from_millis(args.timeout_ms);

    let mut client = target
        .connect(timeout)
        .await
        .map_err(|e| anyhow::anyhow!("connection failed: {e}"))?;
    let init = client
        .initialize()
        .await
        .map_err(|e| anyhow::anyhow!("initialize failed: {e}"))?;
    let tools = client
        .list_tools()
        .await
        .map_err(|e| anyhow::anyhow!("tools/list failed: {e}"))?;

    if !args.quiet {
        eprintln!(
            "connected — {} tool(s) discovered, protocol {}",
            tools.len(),
            init.protocol_version
        );
    }

    let mut rng = StdRng::seed_from_u64(args.seed);
    let mut findings: Vec<Finding> = Vec::new();
    let mut tested = 0usize;

    'fuzz: for tool in &tools {
        if !args.tools.is_empty() && !args.tools.contains(&tool.name) {
            continue;
        }
        let muts = schema::generate_mutations(&tool.input_schema, &mut rng, args.iterations);
        if !args.quiet {
            eprintln!("→ {} ({} payloads)", tool.name, muts.len());
        }

        for m in muts {
            tested += 1;
            match client
                .call_tool_timeout(&tool.name, m.arguments.clone(), timeout)
                .await
            {
                Ok(res) => {
                    if m.category.is_clear_schema_violation() && !res.is_error {
                        findings.push(Finding::accepted_invalid(&tool.name, &m));
                    }
                }
                Err(Error::Timeout) => {
                    findings.push(Finding::hang(&tool.name, &m, args.timeout_ms));
                    match recover(&target, timeout).await {
                        Some(c) => client = c,
                        None => {
                            findings.push(Finding::not_recovered(&tool.name));
                            break 'fuzz;
                        }
                    }
                }
                Err(Error::ConnectionClosed { stderr }) => {
                    findings.push(Finding::crash(&tool.name, &m, stderr));
                    match recover(&target, timeout).await {
                        Some(c) => client = c,
                        None => {
                            findings.push(Finding::not_recovered(&tool.name));
                            break 'fuzz;
                        }
                    }
                }
                Err(Error::Rpc(e)) => {
                    // -32602 (invalid params) and friends mean the server validated input —
                    // that is healthy. Only an internal error (-32603) is a finding.
                    if e.code == -32603 {
                        findings.push(Finding::internal_error(&tool.name, &m, &e));
                    }
                }
                Err(other) => {
                    findings.push(Finding::transport(&tool.name, &m, other.to_string()));
                }
            }
        }
    }

    report::print_summary(&init, &tools, tested, &findings);

    if let Some(p) = &args.sarif {
        std::fs::write(p, sarif::to_sarif(&findings))?;
        if !args.quiet {
            eprintln!("SARIF written to {}", p.display());
        }
    }
    if let Some(p) = &args.json {
        std::fs::write(p, serde_json::to_vec_pretty(&findings)?)?;
        if !args.quiet {
            eprintln!("JSON written to {}", p.display());
        }
    }

    let worst = findings.iter().map(|f| f.severity).max();
    Ok(match worst {
        Some(w) if w >= args.fail_on => 1,
        _ => 0,
    })
}

/// Try to bring a server back after a crash/hang. Returns a live, re-initialized
/// client, or `None` if it did not recover — so the caller stops fuzzing instead
/// of mis-attributing every later payload to a fresh "crash".
async fn recover(target: &Target, timeout: Duration) -> Option<McpClient> {
    let client = target.connect(timeout).await.ok()?;
    client.initialize().await.ok()?;
    client.is_alive().then_some(client)
}
