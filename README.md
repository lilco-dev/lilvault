# lilvault

A self-hosted gateway exposing existing Obsidian Sync vaults to remote AI agents over MCP Streamable HTTP, with a small REST API for diagnostics and automation.

**Status: M0 feasibility passed; the M1 Rust/REST slice is implemented. MCP/Hermes integration and production packaging are next. Use disposable vaults only for now.**

## Progress

M0 verified official Obsidian Headless in Docker on `zoro`: real desktop round trips, two isolated vaults, container recreation, external atomic writes, bounded offline recovery, and observed conflict behavior. M1 adds fail-closed configuration, vault-scoped bearer authentication, Linux descriptor-relative Markdown operations, conditional writes, and a REST API. See [M0 findings](docs/M0-FINDINGS.md) and the [REST contract](docs/API.md).

The intended flow is:

```text
Hermes → authenticated MCP over approved HTTPS/private transport
       → Rust gateway → local vault files → official Obsidian Headless
       → Obsidian Sync → Obsidian client
```

A successful local write will not imply cloud synchronization. Obsidian credentials must remain isolated from the gateway. M0 also found that Headless's default merge can discard one competing edit while reporting success; see the findings log before using real notes.

## Run the REST slice

Requires Linux with `openat2` (kernel 5.6+) and a Rust toolchain. Start with an existing **disposable** local directory, not important notes:

```sh
cp config.example.toml config.toml
# Edit config.toml: set the vault path to your existing absolute directory.
# Keep the loopback listen address unless a secure transport is approved.
export LILVAULT_HERMES_TOKEN="$(openssl rand -hex 32)"
cargo run -- config.toml
```

The server reads exported environment variables, not `.env` files. In another terminal with the same token available:

```sh
curl -H "Authorization: Bearer $LILVAULT_HERMES_TOKEN" \
  http://127.0.0.1:8737/v1/vaults
```

`GET /healthz` and `GET /readyz` are unauthenticated probes. The latter measures local roots, **not** cloud sync. Writes require `If-None-Match: *` for creation or an exact `If-Match` ETag for replacement; blind overwrite is rejected. Read [docs/API.md](docs/API.md) for routes, path restrictions, error codes, and concurrency limits.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

## Project documents

- [SPEC.md](SPEC.md): scope, fixed decisions, security requirements, and acceptance criteria.
- [TASKS.md](TASKS.md): milestone checklist and current blockers.
- [AGENT_PROMPT.md](AGENT_PROMPT.md): kickoff instructions.
- [docs/M0-FINDINGS.md](docs/M0-FINDINGS.md): Headless research and real sync verification.
- [docs/API.md](docs/API.md): REST usage, configuration, filesystem guarantees, and limits.
- [docs/M1-FINDINGS.md](docs/M1-FINDINGS.md): implementation scope and verification evidence.

## Development guardrails

- Use only the official Headless client; do not implement the proprietary Sync protocol.
- Never commit account credentials, bearer tokens, vault content, or sync state. Local data directories and `.env` files are ignored.
- Ask before installing host packages, changing firewall/proxy configuration, or touching existing services.
- Do not expose MCP publicly or assume a hostname or TLS setup.
- The gateway must never mount the sync credential home or Docker socket.
- Back up real vaults; gateway locks do not serialize against external Headless writes.

There is no MCP endpoint, Hermes demo, or production Compose gateway deployment yet. The Headless feasibility containers and the Rust API remain separate; local write success does not establish a cloud round trip.
