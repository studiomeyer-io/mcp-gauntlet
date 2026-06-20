# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-06-20

Initial release. A Cargo workspace of three crates published to crates.io:
`mcp-gauntlet-core`, `mcp-fuzz`, and `mcp-storm`. `#![forbid(unsafe_code)]` in all
three. MSRV 1.86.

### Added

- **`mcp-gauntlet-core`** — async MCP client with concurrent request multiplexing
  over a single stdio pipe (responses demultiplexed by JSON-RPC id) and Streamable
  HTTP. Schema-driven value generator + mutation battery (subset of JSON Schema
  2020-12: type/properties/required/items/enum/const/default/examples/min-max/
  oneOf/anyOf/allOf). In-process mock server for zero-setup demos and hermetic
  tests. Usable standalone as a small MCP client.
- **`mcp-fuzz`** — schema-aware fuzzer CLI. Per tool, fires type-confusion,
  boundary, injection (path-traversal, SQLi, command/template/format-string,
  prompt-injection, CRLF, NUL, RTL/zero-width Unicode — as **data**, never
  executed), missing-required, wrong-typed and deep-nesting payloads. Classifies
  crash (HIGH) / hang (HIGH) / internal-error (MEDIUM) / accepted-invalid (LOW);
  a clean `-32602` is not a finding. Respawns the server after a crash, writes
  SARIF 2.1.0, `--seed` reproducibility, `--fail-on <sev>` CI gate.
- **`mcp-storm`** — load tester CLI. N concurrent workers (real concurrency over
  stdio), p50/p95/p99 + throughput per tool, `--threshold-p95-ms` /
  `--max-error-rate` CI gates, `--warmup-secs` for cold-start exclusion.
- Transports: stdio (subprocess) and Streamable HTTP. HTTP is a default feature
  that compiles out cleanly (`--no-default-features`).

### Security

- Injection payloads are generated and sent strictly as **data** — the fuzzer
  never evaluates, shells out, or reflects them. `#![forbid(unsafe_code)]` is
  global. No `unwrap()`/`expect()` on the client hot path (a panic would kill the
  connection).

[Unreleased]: https://github.com/studiomeyer-io/mcp-gauntlet/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/studiomeyer-io/mcp-gauntlet/releases/tag/v0.1.0
