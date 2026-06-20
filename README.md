# mcp-gauntlet

**A reliability + security toolkit for [Model Context Protocol](https://modelcontextprotocol.io) servers.**
Two single-binary CLIs that share one fast async MCP client core:

| Tool | What it does |
|------|--------------|
| 🔬 **[`mcp-fuzz`](crates/mcp-fuzz)** | Schema-aware fuzzer. Reads each tool's `inputSchema` and throws a battery of hostile/boundary/malformed payloads at it — finds **crashes**, **hangs**, internal errors and silent validation gaps. Emits **SARIF** for GitHub code scanning. |
| 🌩️ **[`mcp-storm`](crates/mcp-storm)** | Load tester ("k6 for MCP"). Drives N concurrent workers against your server, reports **p50/p95/p99** latency + throughput per tool, and **gates CI** on latency/error-rate thresholds. |

Both are written in Rust: one static binary each, no runtime, drop into any CI. They talk MCP over **stdio** (subprocess) or **Streamable HTTP**.

> Built by [StudioMeyer](https://studiomeyer.io). Companion to [`mcp-armor`](https://github.com/studiomeyer-io/mcp-armor) (runtime defense) — `mcp-gauntlet` is the *pre-deploy* attacker + load generator.

---

## Why

MCP servers [fail silently](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/2734) and ship fast. Most have **no tests against malformed input** and **no latency budget**. `mcp-gauntlet` gives you both in two commands you can wire into CI today — without writing a single test by hand, because the payloads are derived from the server's own schema.

---

## Install

```bash
# from crates.io (after release)
cargo install mcp-fuzz mcp-storm

# or from source
git clone https://github.com/studiomeyer-io/mcp-gauntlet
cd mcp-gauntlet && cargo build --release
# binaries in target/release/{mcp-fuzz,mcp-storm}
```

MSRV: Rust 1.82.

---

## `mcp-fuzz` — find crashes before your users do

```bash
# fuzz a stdio server (everything after -- is the launch command)
mcp-fuzz run --stdio -- node my-server.js

# fuzz a remote HTTP endpoint, write SARIF for GitHub code scanning
mcp-fuzz run --http https://example.com/mcp --sarif findings.sarif

# only one tool, more payloads, fail CI on any HIGH finding
mcp-fuzz run --stdio --tool search --iterations 300 --fail-on high -- ./server
```

It connects, calls `tools/list`, and for every tool generates payloads driven by the schema:

- **type confusion** (string where a number is required, …)
- **boundary** (empty / 100k-char strings, `i64::MAX`, negatives, zero, huge floats)
- **injection** (path traversal, SQLi, command/template/format-string, prompt injection, CRLF, NUL bytes, RTL/zero-width Unicode) — sent purely as *data*, never executed
- **missing required fields**, **wrong-typed `arguments`**, **deep nesting**

Outcomes are classified: a dropped connection ⇒ **crash** (HIGH), a timeout ⇒ **hang** (HIGH), `-32603` ⇒ **internal-error** (MEDIUM), success on schema-invalid input ⇒ **accepted-invalid** (LOW). A clean `-32602 invalid params` is treated as the server *correctly* validating — not a finding.

Runs are reproducible (`--seed`). After a crash the fuzzer respawns the server and continues; if the server can't be brought back, it records that once and stops cleanly, rather than blaming every later payload for the original crash.

```text
mcp-fuzz report
───────────────
server         my-server (protocol 2025-11-25)
tools          7
payloads sent  412
findings       2 total — 1 high, 0 medium, 1 low, 0 info

Findings (worst first):
  [HIGH  ] crash            search::server crashed on field 'query': null-byte
           stderr tail:
           thread 'main' panicked at 'invalid utf-8 ...'
  [LOW   ] accepted-invalid search::schema-invalid input accepted without error: missing required field 'query'
```

### In CI (GitHub code scanning)

```yaml
- run: mcp-fuzz run --stdio --sarif mcp.sarif --fail-on high -- node server.js
- uses: github/codeql-action/upload-sarif@v3
  if: always()
  with: { sarif_file: mcp.sarif }
```

---

## `mcp-storm` — know your latency budget

```bash
# 16 workers for 30s against a stdio server
mcp-storm run --stdio --concurrency 16 --duration-secs 30 -- node my-server.js

# fixed request count against HTTP, gate CI on p95 + error rate
mcp-storm run --http https://example.com/mcp --requests 5000 \
  --threshold-p95-ms 250 --max-error-rate 1.0
```

```text
mcp-storm report
────────────────
server       my-server (protocol 2025-11-25)
workers      16
duration     30.00s
requests     48213 (3 errors, 0.01% error rate)
throughput   1607 req/s

tool                    calls    err%     p50ms     p95ms     p99ms     maxms      req/s
──────────────────────────────────────────────────────────────────────────────────────
search                  31044   0.01%      6.20     14.80     31.10    102.40       1035
fetch                   17169   0.00%      9.10     22.30     58.70    210.00        572
```

Over stdio, concurrency is real: requests are multiplexed over the single pipe and demultiplexed by JSON-RPC id, so N workers genuinely overlap. `--threshold-p95-ms` / `--max-error-rate` make it exit non-zero when a budget is blown — drop it straight into a CI gate. Use `--warmup-secs` to exclude cold-start.

---

## Workspace layout

```
mcp-gauntlet/
├── crates/mcp-gauntlet-core   # shared async MCP client (stdio + HTTP), schema gen, mock server
├── crates/mcp-fuzz         # the fuzzer CLI
└── crates/mcp-storm        # the load tester CLI
```

`mcp-gauntlet-core` is published too — it's a small, concurrency-capable MCP client you can use on its own, with a schema-driven value/mutation generator and an in-process [`mock`](crates/mcp-gauntlet-core/src/mock.rs) server (`mcp-fuzz mock` / `mcp-storm mock`) you can point either tool at to try them with zero setup:

```bash
mcp-fuzz run --stdio -- mcp-fuzz mock     # fuzz the bundled demo server
```

## Development

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

## License

MIT © StudioMeyer. See [LICENSE](LICENSE).
