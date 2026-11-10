# M2 — Local MCP slice; remote Hermes demo pending

## Implemented and verified locally

- Official `rmcp` 3.9 Streamable HTTP server at `/mcp`, mounted on the existing Axum listener. Rust 1.88 is the declared minimum because of the SDK; checks ran on the installed 1.97 toolchain.
- SDK-managed initialization, protocol negotiation, input schemas, discovery, JSON/stream transport behavior, and standard tool results.
- Four tools: `list_vaults`, `list_files`, `read_file`, `write_file`, all calling the same ACL/path-safe service as REST.
- Fresh bearer authentication on every request. Only the trusted agent extension is passed into the SDK; the Authorization header is removed. No raw bearer is retained in tool context.
- SDK-supported stateless transport with no legacy session IDs or session/event state shared across credentials. JSON responses are preferred; session resumption is not offered.
- SDK Host validation with explicit configurable authorities; all browser Origins denied. An operator-approved private HTTPS Serve route is now configured and locally verified; no public MCP Funnel or remote Hermes client was configured.
- Consistent structured core errors, required create/revision conditions, 2 MiB note limit, separate escaped JSON envelope limit, and the common request deadline.

See [MCP.md](MCP.md) for the client-facing contract and [API.md](API.md) for shared file/consistency guarantees.

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --bin lilvault
```

All passed: **25 tests** total (18 existing filesystem/config/REST tests plus 7 MCP integration tests). The non-test binary also builds with production dependency features rather than relying on the SDK client test features.

The MCP tests cover:

1. Authenticated legacy-version handshake, discovery of the exact four tools, no session ID issuance, and rejection of an unauthenticated request carrying a forged session header.
2. Per-request credential isolation, hidden unauthorized vault identities, and read-only denial using the shared core.
3. Create/list/read/update, revision propagation, stale/blind/contradictory preconditions, traversal/internal paths, and unchanged content after rejected writes.
4. Denial of every browser Origin (including localhost and `null`) and unapproved Host authorities.
5. Acceptance of an explicitly configured authority, with default/unapproved authorities denied under that custom allowlist.
6. Both Markdown content and independent HTTP-envelope size limits without partial writes.
7. A **real official SDK client** over an ephemeral loopback HTTP server: initialized, discovered tools, wrote a note, read it back, and shut down. All data/config are temporary; existing Sync vaults and containers were not modified by these tests.

The SDK API was checked against installed official source/docs, not copied from an older transport example. Its server configuration is non-exhaustive and must use default/setter initialization. Its current handler macro builds the tool router statically, so an unused stored router was removed instead of silencing Clippy. Active LSP probes were also run; final compiler/Clippy/test/build outputs are the definitive verification.

## Not yet verified / operator input required

**M2 is not complete:** no remote Hermes MCP operation or Hermes-created note's real Obsidian Sync round trip has occurred. The SDK client's temporary-vault test is not a substitute for that acceptance criterion.

The approved private HTTPS :9443 route and loopback gateway are live. The operator provisioned a dedicated `agents` Sync vault through supported interactive setup. Both existing token values were preserved while their permissions changed to `agents:read-write`; real HTTPS checks confirmed that `test` and `test2` are denied. The original Serve/Funnel configuration was compared and remained identical. Runtime files, rollback snapshots, and credential handoff stay outside Git with private permissions. See [TAILNET.md](TAILNET.md).

A create-only setup note was written with one credential and read through the other with a matching SHA-256 revision. Headless reported upload completion with no explicit sync errors. The existing remote helper also discovered `agents` and read that revision successfully using its unchanged credential. This resolves the earlier remote-access blocker for the helper, not the final native Hermes/desktop acceptance criterion. Do not paste token values or encryption passwords into chat or Git.

A private loopback gateway runs under the session process manager using a binary snapshot and only the required credential environment; it is not yet boot-supervised. The gateway has no Obsidian authentication volume or Docker socket. Only the dedicated `agents` sync owner remains active, with Docker `unless-stopped`; retired working trees and authentication storage are retained. Default Headless merge, external-writer CAS limits, unknown API cloud freshness, and timeout uncertainty still apply. Delete/search/supervision and broader conflict/offline hardening remain M3; production Compose/backup/release work remains M4.
