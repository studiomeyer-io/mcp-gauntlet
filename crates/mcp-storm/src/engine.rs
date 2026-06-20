//! The load-test engine: connect, pick tools, drive N concurrent workers, measure.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Args;
use hdrhistogram::Histogram;
use mcp_probe_core::{schema, Error, McpClient};
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde_json::Value;

use crate::report::{self, ToolStats};

/// Arguments for `mcp-storm run`.
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

    /// Number of concurrent workers.
    #[arg(long, default_value_t = 8)]
    pub concurrency: usize,

    /// Run for this many seconds (ignored if --requests is set).
    #[arg(long, default_value_t = 10)]
    pub duration_secs: u64,

    /// Total number of requests to send (overrides --duration-secs).
    #[arg(long)]
    pub requests: Option<u64>,

    /// Only hit this tool (repeatable). Default: all tools, round-robin.
    #[arg(long = "tool", value_name = "NAME")]
    pub tools: Vec<String>,

    /// Per-call timeout in milliseconds.
    #[arg(long, default_value_t = 5000)]
    pub timeout_ms: u64,

    /// RNG seed for the (valid) argument generation.
    #[arg(long, default_value_t = 0x00C0_FFEE)]
    pub seed: u64,

    /// Discard results for this many seconds of warmup before measuring.
    #[arg(long, default_value_t = 0)]
    pub warmup_secs: u64,

    /// Fail (exit 1) if any tool's p95 latency exceeds this many milliseconds.
    #[arg(long, value_name = "MS")]
    pub threshold_p95_ms: Option<f64>,

    /// Fail (exit 1) if any tool's error rate exceeds this percentage (0–100).
    #[arg(long, value_name = "PCT")]
    pub max_error_rate: Option<f64>,

    /// Write a JSON report to this path.
    #[arg(long, value_name = "FILE")]
    pub json: Option<PathBuf>,

    /// Suppress progress output.
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

/// When to stop a measurement phase.
#[derive(Clone, Copy)]
enum Stop {
    Requests(u64),
    Until(Instant),
}

/// Per-tool accumulator returned by each worker.
struct Acc {
    hist: Histogram<u64>,
    count: u64,
    errors: u64,
}

impl Acc {
    fn new() -> Self {
        Acc {
            // 3 significant figures, auto-resizing.
            hist: Histogram::new(3).expect("valid histogram"),
            count: 0,
            errors: 0,
        }
    }
}

struct Shared {
    client: McpClient,
    targets: Vec<(String, Value)>,
    timeout: Duration,
    counter: AtomicU64,
    stop: Stop,
}

/// Run a load test. Returns the process exit code.
pub async fn run(args: RunArgs) -> anyhow::Result<i32> {
    let target = Target::from_args(&args)?;
    let timeout = Duration::from_millis(args.timeout_ms);

    let client = target
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

    // Choose target tools and precompute a valid argument set for each (deterministic).
    let chosen: Vec<_> = tools
        .iter()
        .filter(|t| args.tools.is_empty() || args.tools.contains(&t.name))
        .collect();
    if chosen.is_empty() {
        anyhow::bail!("no matching tools to load test");
    }
    let targets: Vec<(String, Value)> = chosen
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut rng = StdRng::seed_from_u64(args.seed ^ (i as u64).wrapping_mul(0x9E3779B9));
            (
                t.name.clone(),
                schema::valid_value(&t.input_schema, &mut rng),
            )
        })
        .collect();

    if !args.quiet {
        eprintln!(
            "load testing {} tool(s) with {} workers, {}",
            targets.len(),
            args.concurrency,
            match args.requests {
                Some(n) => format!("{n} requests"),
                None => format!("{}s", args.duration_secs),
            }
        );
    }

    // Optional warmup phase (results discarded).
    if args.warmup_secs > 0 {
        if !args.quiet {
            eprintln!("warming up for {}s…", args.warmup_secs);
        }
        let stop = Stop::Until(Instant::now() + Duration::from_secs(args.warmup_secs));
        let _ = run_phase(
            client.clone(),
            targets.clone(),
            args.concurrency,
            timeout,
            stop,
        )
        .await;
    }

    // Measured phase.
    let stop = match args.requests {
        Some(n) => Stop::Requests(n),
        None => Stop::Until(Instant::now() + Duration::from_secs(args.duration_secs)),
    };
    let started = Instant::now();
    let merged = run_phase(
        client.clone(),
        targets.clone(),
        args.concurrency,
        timeout,
        stop,
    )
    .await;
    let elapsed = started.elapsed();

    // Compute stats.
    let mut stats: Vec<ToolStats> = merged
        .into_iter()
        .map(|(tool, acc)| ToolStats::from_acc(tool, &acc.hist, acc.count, acc.errors, elapsed))
        .collect();
    stats.sort_by(|a, b| a.tool.cmp(&b.tool));

    report::print_summary(&init, &stats, elapsed, args.concurrency);

    if let Some(p) = &args.json {
        std::fs::write(p, serde_json::to_vec_pretty(&stats)?)?;
        if !args.quiet {
            eprintln!("JSON written to {}", p.display());
        }
    }

    // CI gating.
    let mut failed = false;
    if let Some(thr) = args.threshold_p95_ms {
        for s in &stats {
            if s.p95_ms > thr {
                eprintln!("GATE FAIL: {} p95 {:.1}ms > {:.1}ms", s.tool, s.p95_ms, thr);
                failed = true;
            }
        }
    }
    if let Some(max) = args.max_error_rate {
        for s in &stats {
            if s.error_rate_pct > max {
                eprintln!(
                    "GATE FAIL: {} error rate {:.2}% > {:.2}%",
                    s.tool, s.error_rate_pct, max
                );
                failed = true;
            }
        }
    }

    Ok(if failed { 1 } else { 0 })
}

/// Run one measurement phase with `concurrency` workers, returning merged per-tool accumulators.
async fn run_phase(
    client: McpClient,
    targets: Vec<(String, Value)>,
    concurrency: usize,
    timeout: Duration,
    stop: Stop,
) -> HashMap<String, Acc> {
    let shared = Arc::new(Shared {
        client,
        targets,
        timeout,
        counter: AtomicU64::new(0),
        stop,
    });

    let mut handles = Vec::with_capacity(concurrency.max(1));
    for w in 0..concurrency.max(1) {
        let shared = shared.clone();
        handles.push(tokio::spawn(async move { worker(shared, w).await }));
    }

    let mut merged: HashMap<String, Acc> = HashMap::new();
    for h in handles {
        if let Ok(local) = h.await {
            for (tool, acc) in local {
                let entry = merged.entry(tool).or_insert_with(Acc::new);
                let _ = entry.hist.add(&acc.hist);
                entry.count += acc.count;
                entry.errors += acc.errors;
            }
        }
    }
    merged
}

async fn worker(shared: Arc<Shared>, worker_id: usize) -> HashMap<String, Acc> {
    let mut local: HashMap<String, Acc> = HashMap::new();
    let mut i = worker_id;

    loop {
        // Termination check.
        match shared.stop {
            Stop::Requests(budget) => {
                if shared.counter.fetch_add(1, Ordering::Relaxed) >= budget {
                    break;
                }
            }
            Stop::Until(deadline) => {
                if Instant::now() >= deadline {
                    break;
                }
            }
        }

        let (tool, args) = &shared.targets[i % shared.targets.len()];
        i = i.wrapping_add(1);

        let start = Instant::now();
        let result = shared
            .client
            .call_tool_timeout(tool, args.clone(), shared.timeout)
            .await;
        let micros = start.elapsed().as_micros().min(u64::MAX as u128) as u64;

        let acc = local.entry(tool.clone()).or_insert_with(Acc::new);
        acc.count += 1;
        match result {
            Ok(_) => {
                let _ = acc.hist.record(micros.max(1));
            }
            Err(_) => {
                acc.errors += 1; // errors are counted but excluded from latency percentiles
            }
        }
    }

    local
}
