# Security Policy

## Reporting a vulnerability

Please report security issues privately to **security@studiomeyer.io** or via GitHub's
private vulnerability reporting ("Report a vulnerability" in the Security tab). We aim to
acknowledge within 72 hours.

## Scope & intent

`mcp-probe` is a **testing tool for servers you are authorized to test.** `mcp-fuzz` sends
deliberately hostile payloads (path traversal, injection strings, oversized input, control
characters). These are transmitted **only as JSON-RPC tool arguments — they are never
executed by `mcp-probe` itself**; any effect is entirely up to the server under test.

Only run `mcp-fuzz` / `mcp-storm` against MCP servers you own or have explicit permission to
test. `mcp-storm` generates load and can overwhelm an under-provisioned target; do not point
it at third-party production endpoints.

## Safety properties

- `#![forbid(unsafe_code)]` across all crates.
- No payload is ever shelled out, `eval`'d, or written to disk by the tools.
- `--seed` makes every run reproducible for triage.
