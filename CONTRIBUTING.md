# Contributing to mcp-gauntlet

Thanks for considering a contribution. `mcp-gauntlet` attacks and load-tests MCP
servers, so the bar for new code is "it reproduces a real failure (or a real
metric) against the bundled mock server, and it ships with a test". Payloads are
always **data** — nothing in this repo ever executes what it generates.

## Quick Start

```sh
git clone https://github.com/studiomeyer-io/mcp-gauntlet
cd mcp-gauntlet
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets --no-default-features -- -D warnings   # http off
cargo test --all-features --workspace
cargo deny check                                  # advisories + licenses + sources
# try it with zero setup against the bundled mock:
cargo run -p mcp-fuzz -- run --stdio -- cargo run -p mcp-fuzz -- mock
```

MSRV is **Rust 1.82** — CI checks it on a pinned 1.82 toolchain plus stable. Your
patch needs to compile on the floor.

## What we accept

- **New fuzz mutation classes.** A new hostile/boundary/injection generator in
  `mcp-gauntlet-core/src/schema.rs`, plus a test that makes the bundled `fragile`
  mock server produce a deterministic finding. Generators emit data only.
- **New storm metrics / gates.** Throughput, percentiles, budget flags — each with
  a `selftest.rs` assertion.
- **Transport support / client improvements** in `mcp-gauntlet-core`.
- **Bug fixes.** A failing test in your PR description is the fastest path to merge.
- **Docs.** Typo fixes, clarifications, ecosystem links.

## What we are slow on

- **Executing payloads.** Non-negotiable: the fuzzer sends injection strings as
  JSON values and never evaluates them. PRs that shell out, render, or `eval`
  generated input will be declined.
- **Runtime dependencies.** Every crate added is a supply-chain surface for a
  security tool — `cargo deny` gates licenses + sources, and we weigh
  maintainership before accepting a new dep.
- **Scope creep** beyond pre-deploy fuzzing + load testing. Runtime defense lives
  in the sister project [`mcp-armor`](https://github.com/studiomeyer-io/mcp-armor).

## Pull Request Process

1. Open an issue or draft PR first for anything non-trivial.
2. One logical change per PR. Easier to review, easier to revert.
3. CI must be green: `fmt --check`, `clippy -- -D warnings` (all-features **and**
   `--no-default-features`), `test --workspace`, the MSRV-1.82 check, and
   `cargo deny check`.
4. Add a `CHANGELOG.md` entry under `[Unreleased]` in plain English.
5. For security-impacting changes, see [SECURITY.md](SECURITY.md) — please email
   instead of opening a public issue.

## Coding Standards

- `#![forbid(unsafe_code)]` is global. There are no exceptions.
- No `unwrap()` / `expect()` on the client hot path. The core runs an async
  request/response loop over a single pipe; a panic kills the whole connection.
  Propagate errors or use a sane fallback.
- Clippy is `-D warnings`, all-targets, both feature sets.
- `rustfmt` with the repo's `.rustfmt.toml` (edition 2021, 100 cols).

## Testing

- Unit tests live next to the code in `#[cfg(test)] mod tests`.
- Integration tests live in each binary crate's `tests/selftest.rs` and run
  against the in-process mock server for deterministic, network-free results.
- A new fuzz finding class needs a mock-server case that produces it.

## Releasing (maintainers)

- Bump `version` in the workspace `Cargo.toml` and add a dated `CHANGELOG.md`
  section.
- Tag `vX.Y.Z` on `main`. `release.yml` publishes the three crates to crates.io in
  dependency order (`-core` → `mcp-fuzz` → `mcp-storm`); needs the
  `CARGO_REGISTRY_TOKEN` repo secret.
- After release, verify on crates.io and via `cargo install mcp-fuzz mcp-storm`.

## License

By contributing, you agree your work is licensed under the [MIT License](LICENSE).

## Code of Conduct

Be kind. Assume good faith. We are a small studio in Palma de Mallorca — no drama,
disagreement is fine, contempt is not.
