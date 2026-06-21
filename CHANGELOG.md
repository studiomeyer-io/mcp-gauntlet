# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **`mcp-fuzz` / `mcp-gauntlet-core`: false-positive `accepted-invalid` findings on
  union- and nullable-typed fields.** Type-confusion mutations read only the first
  string `type` and fell back to `string` for the union form `"type": ["string","null"]`,
  so they could feed a *conforming* value (e.g. an integer to a `["integer","string"]`
  field) and then flag the server for "accepting schema-invalid input". The mutator now
  resolves the full declared type set, only emits a type-confusion payload that is
  genuinely outside it (skipping untyped/any fields entirely), and carries a per-mutation
  `Mutation::clear_violation` flag so acceptance is reported only when the input is an
  unambiguous violation — a required field with no `default`. Union/nullable string
  fields now also receive the full boundary + injection battery they previously missed.
- **`mcp-gauntlet-core`: `gen_number` ignored `maximum` and `multipleOf`.** It could
  return a value above the declared `maximum` (when only `minimum` was set, contradictory
  bounds) and never honoured `multipleOf`, unlike `gen_integer`. Brought to parity:
  clamps to the upper bound and rounds up to a conforming multiple where the bounds allow.
- **`mcp-gauntlet-core` (mock): the `slow` tool collapsed to a 0 ms sleep.** Its `ms`
  schema had only `minimum: 0`, so the value generator produced `0` and the tool could
  not drive latency. Added `default: 5` so load tests and timeout self-tests get a real,
  deterministic sleep.

### Changed

- CI GitHub Actions bumped (supersedes Dependabot #1): `actions/checkout` 4 → 7,
  `github/codeql-action/upload-sarif` 3 → 4, `actions/upload-artifact` 4 → 7. The MSRV
  job's `dtolnay/rust-toolchain` pin is intentionally kept at **1.86.0** (Dependabot
  proposed 1.100.0, which would have made the MSRV check verify a newer toolchain and
  silently mask MSRV regressions).

### Tests

- Added 24 tests (21 → 45): union/nullable type-confusion no-false-positive,
  `clear_violation` semantics, `gen_number` bounds/`multipleOf`, untyped-field skipping;
  `mcp-storm` percentile math (empty / single-sample / p50≤p95≤p99 ordering / error-rate
  / throughput) and CI-gate behaviour in **both** directions (a healthy run under generous
  thresholds passes → exit 0; timed-out calls trip the error-rate gate → exit 1);
  `mcp-fuzz` SARIF 2.1.0 structure + level enum + stable fingerprints, a written-to-disk
  SARIF check, and hang-vs-crash classification under a tight timeout; JSON-RPC id
  coercion and SSE id-mismatch safety in the client.

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
