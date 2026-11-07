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

The operator identified the remote Hermes Tailnet machine and approved separate private HTTPS on port 9443. The route and loopback gateway are now live; normal-TLS authentication and the `test:read-write` scope were verified from the deployment host. Original port-443 Funnel entries were compared and preserved, with no new public Funnel. A dedicated random token and runtime files were provisioned outside Git with private permissions. See [TAILNET.md](TAILNET.md).

The first strict read-only SSH attempt to the remote machine on port 22 timed out, and Tailnet status subsequently reported it offline. Remote profile/version inspection, Hermes configuration, secure credential transfer, and actual client access remain blocked; no remote configuration or service changes were made. Need the machine online and the correct approved SSH user/port, or an operator-applied configuration path. Do not paste token values into chat or Git.

A private loopback gateway now runs under the session process manager using a binary snapshot; it is not yet boot-supervised. The gateway has no Obsidian authentication volume or Docker socket. The M0 sync processes remain separate. Default Headless merge, external-writer CAS limits, unknown cloud freshness, and timeout uncertainty still apply. Delete/search/supervision and broader conflict/offline hardening remain M3; production Compose/backup/release work remains M4.
