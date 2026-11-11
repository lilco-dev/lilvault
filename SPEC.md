# lilvault — Implementation Specification

Version: 0.1.0-draft\
Target: Linux host (operate directly when local, otherwise via SSH)\
Implementation: Rust, Docker Compose, Obsidian Headless\
Status: Ready for implementation, subject to M0 validation

## 1. Mission

Build **lilvault**, a focused, self-hostable gateway that exposes existing Obsidian Sync vaults to remote AI agents over MCP (Streamable HTTP), with a small REST API for debugging and automation. Agents must not install Obsidian, run the Obsidian CLI, or maintain local synchronized vaults.

**First success:** Hermes, running remotely, connects to lilvault, reads a note, writes a new Markdown note, and the note appears in a normal Obsidian client through Obsidian Sync.

Prioritize minimal setup, operational reliability, and correct file semantics over feature breadth.

## 2. Fixed decisions

- Rust gateway: Tokio + Axum + Serde + tracing. Prefer `rmcp` for MCP after verifying current API and Streamable HTTP support.
- Docker Compose deployment on the deployment host.
- Multiple Obsidian Sync vaults, each with its own persistent local working tree and independent Headless sync process.
- Official `obsidian-headless` package for synchronization; **do not reverse engineer the proprietary Sync protocol**.
- Obsidian Sync subscription is required.
- One lilvault instance; no distributed coordination or S3.
- MCP is the primary agent interface; REST is a complementary diagnostic and automation interface.
- Configuration-driven vault provisioning in v0.1; no browser admin UI.
- Per-agent bearer credentials, vault-scoped read/read-write permissions.
- Use existing filesystem as the source of local file content. SQLite may be added for search/indexing and token metadata, but not as a second authoritative document store.

## 3. Non-goals

No hosted signup, agent orchestration, message board, embeddings, knowledge graph, OAuth, Obsidian plugin, Git backend, Cloudflare Workers, alternative Sync protocol, collaborative real-time editing, automatic merge engine, or attachment editing in v0.1.

## 4. User stories

1. As operator, I can connect an existing Obsidian Sync vault once on the deployment host.
2. As operator, I can configure two or more vaults without rebuilding the application.
3. As an agent, I can discover authorized vaults, list directories, read Markdown notes, create notes, and update notes through MCP.
4. As an agent, I cannot access vaults for which my credential lacks permission.
5. As a human, I see agent-created notes in Obsidian after sync completes, and agents can read my Obsidian edits after they synchronize to the deployment host.
6. As operator, I can identify when sync is degraded rather than mistake local write success for cloud synchronization.
7. As operator, I can restart the deployment without losing vaults or credentials.

## 5. Architecture

```text
Remote agents (Hermes, Claude, etc.)
            |
          HTTPS
            |
Reverse proxy / private tunnel (existing infrastructure)
            |
      Rust lilvault server
       |             |
     MCP          REST API
       \             /
       Shared vault service
         |       |
     Auth/ACL   Path-safe file I/O
                  |
       Persistent vault directories
                  |
       Obsidian Headless sync container
         (one process per vault)
                  |
             Obsidian Sync
                  |
           Obsidian clients
```

Use two long-running services: the Rust API and `sync` (Node runtime with the official Headless CLI). Share only vault data between them. Sync authentication storage belongs to Sync and must not be mounted into the gateway. Both run as non-root with compatible ownership; TrueNAS deployments default to the built-in apps UID/GID 568:568. A bounded one-shot, network-disabled initializer may prepare only the dedicated dataset directories, generated non-secret config, and runtime-cache ownership before these services start.

**Sync ownership:** one Headless process per vault working directory; never start two processes against the same path. Docker Compose restarts the sync container. A minimal supervisor may launch and monitor multiple child processes, restarting failed children with bounded exponential backoff. Avoid a complex orchestration system.

## 6. Repository structure

```text
lilvault/
  Cargo.toml
  Cargo.lock
  crates/
    lilvault-core/        # vault service, path validation, file operations, authorization
    lilvault-server/      # Axum, configuration, health, REST, process bootstrap
    lilvault-mcp/         # MCP tools calling lilvault-core
  deploy/
    Dockerfile            # Rust gateway
    Dockerfile.sync       # pinned Obsidian Headless runtime
    compose.yaml
    sync-entrypoint.sh
  scripts/
    bootstrap-host.sh
    verify-sync.sh
  docs/
    OPERATIONS.md
    SECURITY.md
  tests/
  config.example.toml
  .env.example
  README.md
  SPEC.md
  TASKS.md
```

Prefer a small workspace. Collapse crates if workspace complexity slows the first vertical slice; preserve module boundaries regardless.

## 7. Configuration

Config file with environment-based secrets, e.g.:

```toml
[server]
listen = "0.0.0.0:8737"

[[vaults]]
id = "personal"
path = "/data/vaults/personal"
enabled = true

[[vaults]]
id = "work"
path = "/data/vaults/work"
enabled = true

[[agents]]
name = "hermes"
token_env = "LILVAULT_HERMES_TOKEN"
permissions = ["personal:read-write", "work:read"]
```

Configuration is illustrative; implement a documented, validated schema. Never print secret values. Require nonempty secrets of adequate entropy; store hashes if persistent token management is introduced. Fail closed on malformed configuration, unknown vaults, duplicate IDs, missing tokens, and inaccessible paths. Vault paths must be explicitly configured; agents cannot create arbitrary mounts.

## 8. MCP contract

Use MCP Streamable HTTP with a stable `/mcp` endpoint. Support the MCP SDK's required initialization, session, and transport semantics; do not invent a bespoke JSON-over-HTTP protocol. Validate request authorization on every operation, including subsequent requests within a session. Validate Origin where applicable and use a compatible remote-agent authentication approach.

Required tools:

| Tool | Inputs | Outputs |
|---|---|---|
| `list_vaults` | none | authorized vault IDs and access levels |
| `list_files` | `vault_id`, `path` (default root) | immediate child paths, type, size |
| `read_file` | `vault_id`, `path` | UTF-8 content, revision token |
| `write_file` | `vault_id`, `path`, `content`, `expected_revision` OR create-only flag | new revision, local write result |
| `delete_file` | `vault_id`, `path`, `expected_revision` | deletion result |
| `search_files` | `vault_id`, `query`, optional `limit` | matching note paths and snippets |
| `get_vault_status` | `vault_id` | local availability, sync process state, last observed successful sync if measurable |

`write_file` must require either a matching revision for updates or an explicit create-only condition; blind overwrite is prohibited. Errors should be structured, clear, and actionable. Keep tool descriptions concise and agent-friendly.

For the first vertical slice, implement `list_vaults`, `list_files`, `read_file`, and `write_file`; add the remaining tools after end-to-end validation.

## 9. REST contract

Version endpoints under `/v1`:

- `GET /healthz`: process liveness; no secrets.
- `GET /readyz`: gateway initialized and local vault paths available; distinguish sync readiness separately.
- `GET /v1/vaults`: only vaults authorized for caller.
- `GET /v1/vaults/{vault}/status`: sync/local status.
- `GET /v1/vaults/{vault}/files?path=...`: directory listing.
- `GET /v1/vaults/{vault}/files/{*path}`: read content, ETag header.
- `PUT /v1/vaults/{vault}/files/{*path}`: Markdown write, conditional headers.
- `DELETE /v1/vaults/{vault}/files/{*path}`: conditional deletion.
- `GET /v1/vaults/{vault}/search?q=...`: search.

Define unambiguous handling for path encoding and empty paths. API responses should use consistent JSON errors. For writes, require `If-None-Match: *` for creation or `If-Match` for replacement; return 428 for missing preconditions and 412 for mismatches. Return 403 for insufficient permission, 404 for nonexistent resources (without leaking unauthorized vault identities), 409 for detected sync conflicts, and 503 when vault is not safely writable.

File content may be returned as `text/markdown` for REST reads with metadata in headers; MCP returns content and revision together. Document exact behavior in OpenAPI or README examples.

## 10. Filesystem and concurrency rules

- Only `.md` files may be created/modified/deleted in v0.1. Other regular files may be read in a future release; initial MCP `read_file` handles UTF-8 Markdown only.
- Reject absolute paths, `..`, NUL, symlinks (including parent-directory symlinks), reserved internal files, and `.obsidian` access. Ensure path checks are safe against symlink TOCTOU races; use descriptor-relative safe operations where supported, or fail closed and document residual limitations.
- Enforce file size limits (default 2 MiB for Markdown), maximum search results, and request timeouts.
- Hash file bytes with SHA-256 for ETags/revision tokens. Do not rely solely on mtime.
- Serialize writes to the same file through the gateway; re-check the revision while holding the lock. Write a temporary file in the same directory, fsync as appropriate, then atomically rename; clean up temporary files.
- **Important:** the Headless sync process is an external writer and bypasses gateway locks. Gateway conditional writes do not guarantee atomic compare-and-swap against Headless changes. Detect mismatches before commit where possible, test races, and document this as a bounded guarantee; do not claim distributed linearizability.
- Do not automatically merge conflicting notes. Let the underlying Sync system manage remote conflicts and expose degraded/conflict state where observable.
- File write success means the local filesystem accepted the write, **not** that Obsidian Sync acknowledged it. Expose this distinction in responses and docs.

## 11. Search

Start with filename and content substring search across authorized Markdown notes. For a modest vault, a bounded filesystem scan is acceptable for the vertical slice. Add SQLite FTS5 after end-to-end sync works if needed. If indexed, rebuild or reconcile the index after external sync changes; index lag must not be misrepresented as authoritative freshness. Never index `.obsidian`, secrets, or disallowed paths. Search must enforce vault authorization before returning paths or snippets.

## 12. Sync integration

Use the official Obsidian Headless package without modifying or extracting proprietary internals. Pin an exact package version and Node runtime version. Verify current official documentation and actual `ob --help` output before scripting commands. In particular verify:

- Login and credential persistence location in the container.
- Remote vault enumeration and setup syntax.
- Ability to map multiple remote vaults to distinct local directories.
- One-shot and continuous sync behavior.
- External filesystem change detection.
- Process exit codes, failure logs, and observable sync status.
- Encryption/password prompts and unattended restart behavior.
- Licensing and terms applicable to self-hosted use.

Credentials should be provisioned interactively or via supported documented environment variables, never checked into Git. Use a dedicated, persistent credentials volume. Initial setup may require SSH; the running service must not require an interactive terminal.

Sync status must distinguish `initializing`, `ready`, `degraded`, and `stopped`, with explicit `unknown` fields when the Headless client offers no reliable acknowledgment. Do not infer cloud freshness from process liveness alone.

## 13. Deployment on the deployment host

Use `deploy/compose.yaml` with:

- `lilvault` Rust service bound to `127.0.0.1:8737` on host (not public by default).
- `sync` service with pinned Node and official Headless client. Public GHCR images contain only a runner; it installs the pinned official package at runtime into a separate persistent cache instead of assuming redistribution rights to the proprietary client.
- Compose-driven directory/config initialization and GHCR version/SHA/latest tags, without manual NAS builds or directory preparation.
- Persistent vault, state, and sync-credential directories.
- Non-root users and compatible UID/GID.
- Restart policies, graceful termination, health checks where meaningful.
- No Obsidian credentials in the gateway container.
- No external DB required.

Provide a documented route for remote MCP over HTTPS through an existing reverse proxy or secure tunnel; **do not assume the deployment host has a domain, TLS certificates, or a particular proxy installed**. Ask the operator which exposure mechanism to use before modifying network or proxy configuration. Never expose unauthenticated MCP or vault data publicly.

Suggested data layout:

```text
~/lilvault/data/
  vaults/personal/
  vaults/work/
  state/
  obsidian-auth/
```

The implementation agent may inspect the deployment host over SSH but must request approval before installing system packages, changing firewall/proxy settings, or touching existing non-lilvault services. Do not overwrite existing vault directories. Back up before destructive operations.

## 14. Tests and acceptance criteria

### Unit tests
- Path traversal and encoded traversal rejection.
- Symlink and nested symlink rejection.
- Vault authorization matrix, including read-only tokens.
- ETag precondition handling and missing/stale revisions.
- Markdown-only write restrictions and size limits.
- Consistent errors for REST and MCP.

### Integration tests
- Start API against temporary vaults; create/read/update/delete notes.
- Two API clients racing to update the same revision: exactly one succeeds when both go through gateway.
- Multiple vaults isolated from each other.
- MCP handshake and tool calls with an actual MCP client.
- Container restart retains files and configuration.

### Manual end-to-end tests
- Authenticate Headless to a disposable Obsidian Sync vault.
- Create a note in Obsidian and observe it on the deployment host.
- Create a note via Hermes MCP and observe it in Obsidian.
- Restart both containers; no re-login required.
- Add second vault; verify independent sync and access control.
- Disconnect network; verify degraded status and no false cloud-sync acknowledgment.
- Test simultaneous Obsidian/agent edits and document observed conflict behavior.

**Definition of done:** Hermes can read and write an existing Obsidian vault from a remote MCP client without any Obsidian software installed in Hermes's environment; the edit synchronizes to Obsidian; multiple vaults remain isolated; deployment survives restart; setup is documented and reproducible.

## 15. Milestones

**M0 — Validate Headless on the deployment host (hard gate).** Inspect host; build minimal pinned Headless container; authenticate to disposable vault; verify bidirectional sync, restart, second vault, and external writes. Record exact commands, version, and results. Do not build a substantial API before this passes.

**M1 — Minimal Rust gateway.** Implement config, auth, path-safe filesystem core, REST read/list/write, revision checks, tests.

**M2 — MCP vertical slice.** Implement MCP list/read/write, connect Hermes, demonstrate note round-trip in Obsidian.

**M3 — Multi-vault hardening.** Vault ACLs, deletion, status, search, multi-process sync supervision, degraded behavior, integration tests.

**M4 — Packaging and documentation.** Compose deployment, setup scripts, operations/security docs, backup/recovery instructions, fresh-install verification.

## 16. Coding agent operating instructions

1. Start by inspecting the existing the deployment host environment and report findings. Do not assume Docker, Rust, Node, reverse proxy, or repository directory are already present.
2. Verify official Headless CLI behavior against the installed pinned version; do not fabricate command flags or credential paths.
3. Implement **M0 first**. If blocked, report the blocker and a minimal alternative; do not silently substitute an unofficial sync protocol.
4. Commit working changes in small, logically separated steps. Do not commit credentials, local vault contents, database files, or `.env`.
5. Run `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` when Rust code exists. Run integration tests and show their results.
6. Maintain `TASKS.md` with checkboxes, completed milestones, blockers, and exact verification commands.
7. Prefer a minimal end-to-end working slice over speculative abstractions.
8. Before claiming success, demonstrate a real MCP client interacting with a vault and a real Obsidian Sync round trip.
9. Never claim a local write is remotely synchronized unless that has been observed or explicitly acknowledged by a supported mechanism.
10. Ask the operator before destructive operations, public exposure, or installing/changing host-level infrastructure.

## 17. Deferred research

A Cloudflare Workers + Durable Objects implementation that directly speaks the Obsidian Sync protocol is a separate future research project. It must not expand the v0.1 scope or delay the Docker-based utility.
