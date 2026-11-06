# M1 — Rust filesystem and REST

## Scope

One small Cargo package, with separate config, service/auth, vault filesystem, error, and HTTP modules. The spec explicitly permits collapsing crates for the first vertical slice; MCP will call the same service rather than duplicating file or permission logic.

Implemented:

- Strict TOML registry with environment-only, per-agent bearer secrets and read/read-write permissions.
- Startup validation for unknown fields/scopes, duplicate identifiers/secrets/permissions, invalid or missing tokens, unsafe/inaccessible roots, and overlapping or physically aliased vault directories.
- Hashed bearer comparison, authorized-only vault discovery, hidden unauthorized identities, and core-layer permission enforcement.
- Linux `openat2` resolution anchored to directory descriptors, rejecting traversal, symlinks, nested mounts, internal/hidden paths, and hard-linked notes. No insecure filesystem fallback.
- Markdown-only UTF-8 reads/writes, bounded sizes/listings, sorted child listings, SHA-256 revisions, and existing-parent-only writes.
- Per-vault write serialization, revision re-checks before commit, hidden same-directory temporary files, fsync, atomic rename, create-only no-replace commit, and failure cleanup.
- Axum liveness/readiness, vault listing, directory listing, raw Markdown reads, conditional writes, consistent JSON errors, request deadline, and graceful SIGTERM/Ctrl-C.
- Local success explicitly reports `cloud_sync: unknown`; sync credentials/containers are not part of the Rust runtime.

The exact request/response and path contract is in [API.md](API.md). No existing Obsidian notes or running sync containers were modified by M1 verification.

## Automated verification

Commands:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Regression coverage comprises 18 tests (3 filesystem tests, 4 configuration tests, 11 REST tests), with additional cases asserted within each:

- Traversal, encoded/double-encoded traversal, absolute paths, NUL/control characters, hidden/internal paths, and non-Markdown rejection.
- Final/parent symlinks, hard links, and concurrent directory-to-symlink swaps; outside files remain unchanged.
- ACL/read-only isolation, unknown-vault non-disclosure, duplicate authorization header rejection, and authenticated discovery.
- Create/read/update lifecycle, exact content ETags, required/malformed/stale preconditions, and two clients racing the same revision (one 200, one 412).
- External write/creation detected before commit, with abandoned temporary-file cleanup and no overwrite.
- UTF-8 and 2 MiB limits, filtered listings, unavailable/replaced roots, and stable JSON errors.
- Invalid/missing/duplicate config secrets, registry/schema errors, overlapping roots, and non-echoing error messages.

Active LSP probes reported no findings, but the final push-only server checks were inconclusive rather than confirmed clean. Compiler/Clippy and test runs are the authoritative checks. A stale missing-`http`-module report emitted before the file existed was explicitly marked stale after successful compilation and a fresh probe.

## Actual HTTP smoke test

A bounded Python standard-library test launched the compiled binary against a temporary vault on `127.0.0.1:0` with a fresh random bearer injected through its environment, not command arguments. It exercised real HTTP liveness/readiness, denied unauthenticated discovery, created/read/updated a Markdown file, verified its content hash/ETag, rejected stale/missing preconditions and encoded traversal, and checked bytes on disk. SIGTERM shut down cleanly with exit 0. The process, temporary config/data, and random credential were removed; no development server was left running.

## Remaining limits and next milestone

M1 serializes gateway writes, not Headless writes. The final external check/rename race and timed-out-write uncertainty are documented, not hidden. External tools can rename already-open directories; this does not defend against a malicious host administrator. Cloud freshness remains unknown and default Headless merge may drop competing edits, as observed in M0.

M2 must verify the supported `rmcp` Streamable HTTP API, implement authenticated MCP calls using the shared service, test a real MCP client, and arrange approved remote Hermes transport before any exposure. Delete/search/sync supervision and deeper production/offline hardening remain M3; Docker gateway packaging, backups, and clean-install verification remain M4.
