# MCP Streamable HTTP

The gateway exposes the official `rmcp` 3.9 transport at **`/mcp`** on the same listener as REST. Rust 1.88+ is declared for the SDK. No bespoke JSON-RPC transport or proprietary Sync implementation is used.

## Transport and authorization

Configure a remote MCP client with the Streamable HTTP URL and `Authorization: Bearer <agent-token>`. The same strict token configuration and vault-scoped service used by REST applies. Every HTTP request is authenticated, including initialization, discovery, notifications, and later calls. The bearer header is removed after authentication; handlers receive only a trusted per-request agent extension, not the raw token. Permission checks run again in the shared service for each tool operation.

The server uses the SDK's **stateless** mode (`NeverSessionManager`, `legacy_session_mode=false`) and JSON preference. Initialization and version/transport semantics remain SDK-managed. No session ID is issued and no credential is bound permanently to a reusable handler. Requests cannot reuse another credential's legacy session or SSE event history. This does not offer resumable session/event streams; clients must support standard stateless Streamable HTTP. Simple calls use JSON; SDK-required stream/error behavior is not replaced by a custom protocol.

The SDK validates `Host`; defaults are `localhost`, `127.0.0.1`, and `::1`. For an approved HTTPS reverse proxy or private tunnel, explicitly set its authority under `[server]`:

```toml
mcp_allowed_hosts = ["127.0.0.1", "localhost", "your-approved-host.example:443"]
```

This is an illustrative hostname, not a provisioned endpoint. Wildcards, URLs, and whitespace are rejected in this setting; changing it does not provision TLS or authorize public exposure. Keep the gateway on loopback until transport is agreed.

**Browser clients are not enabled:** every present `Origin` value, including `null` and localhost origins, is rejected by the SDK. Non-browser Hermes/agent clients normally omit Origin. There is no CORS or OAuth flow. Invalid bearer requests receive 401 before SDK dispatch; unauthorized/unknown vault identities remain indistinguishable at the tool layer. SDK transport/protocol errors retain SDK semantics, while application errors use structured tool results.

## Tools

| Tool | Arguments | Structured result |
|---|---|---|
| `list_vaults` | `{}` | `vaults`: authorized IDs and `read`/`read-write` access |
| `list_files` | `vault_id`, optional `path` (root by default: omit `path` or pass an empty string; `"/"` is rejected) | `files`: immediate visible Markdown files/directories, path/type/size |
| `read_file` | `vault_id`, `path` | `content`, unquoted SHA-256 `revision` |
| `write_file` | `vault_id`, `path`, `content`, **either** `create_only: true` **or** `expected_revision` | `revision`, `local_write: true`, `cloud_sync: "unknown"` |

A revision is exactly 64 lowercase hex characters, not a quoted HTTP ETag. `create_only: false` with no revision is rejected; a revision and `create_only: true` together are rejected. Unknown argument fields are denied. Blind overwrite is never inferred. Tool descriptions, input schemas, tool discovery, and initialization are supplied through the MCP SDK.

Application errors have `isError: true` and structured content like:

```json
{"error":{"code":"precondition_failed","message":"revision precondition did not match"}}
```

They preserve the core/REST error codes. Invalid argument shapes may instead be SDK invalid-params protocol errors. Do not retry a rejected write without rereading or fixing its condition. File and path restrictions, single-instance write serialization, external-writer races, temporary-file durability, timeout uncertainty, and default Sync merge limitations are exactly those in [API.md](API.md).

Markdown content is capped at 2 MiB. The MCP HTTP envelope is separately capped at `6 * 2 MiB + 64 KiB` to allow worst-case JSON escaping plus protocol metadata; this does not increase the note limit. The common HTTP deadline remains 30 seconds. Readiness and write success do not acknowledge cloud synchronization. Server logging defaults suppress SDK debug/trace request dumps; do not enable payload tracing on sensitive vaults.

## Verification and remote demo boundary

Automated tests use temporary vaults, a real official SDK MCP client over an ephemeral loopback HTTP listener, and explicit protocol requests for authentication, host/origin checks, credential isolation, preconditions, and size/path rejection. These checks do not establish compatibility with an unconfigured remote Hermes installation or prove an MCP-created note reached desktop Obsidian.

Before the real Hermes demo, agree the remote client location, approved HTTPS/private transport, allowed Host authority, and a dedicated vault-scoped random agent token. Do not alter the existing Tailscale Funnel/proxy route or another service without approval. Gateway deployment must never mount the sync credential home or Docker socket.
