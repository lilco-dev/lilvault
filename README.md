# lilvault

A self-hosted gateway exposing existing Obsidian Sync vaults to remote AI agents over MCP Streamable HTTP, with a small REST API for diagnostics and automation.

**Status: M0 feasibility investigation; no gateway or deployment is implemented yet.**

## First milestone

Prove the official Obsidian Headless client works reliably in Docker on `zoro`, using disposable Sync vaults. Validate bidirectional edits, external filesystem writes, restart persistence, independent vault processes, conflicts, and offline behavior **before building the Rust gateway**.

The intended flow is:

```text
Hermes → authenticated MCP over approved HTTPS/private transport
       → Rust gateway → local vault files → official Obsidian Headless
       → Obsidian Sync → Obsidian client
```

A successful local write will not imply cloud synchronization. Obsidian credentials must remain isolated from the gateway.

## Project documents

- [SPEC.md](SPEC.md): scope, fixed decisions, security requirements, and acceptance criteria.
- [TASKS.md](TASKS.md): milestone checklist and current blockers.
- [AGENT_PROMPT.md](AGENT_PROMPT.md): kickoff instructions.
- [docs/M0-FINDINGS.md](docs/M0-FINDINGS.md): research, verification evidence, and next steps.

## Development guardrails

- Use only the official Headless client; do not implement the proprietary Sync protocol.
- Never commit account credentials, bearer tokens, vault content, or sync state. Local data directories and `.env` files are ignored.
- Ask before installing host packages, changing firewall/proxy configuration, or touching existing services.
- Do not expose MCP publicly or assume a hostname or TLS setup.
- Keep M0 blocked until actual synchronization and restart tests pass; documentation research alone is not acceptance.

There is no runnable quickstart yet. Follow the M0 findings log for the current prerequisites rather than treating the planned architecture as working software.
