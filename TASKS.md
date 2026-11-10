# lilvault — Implementation Tasks

## Current status

**M1, the portable CLI, and the local M2 MCP slice are implemented and verified; independent desktop visibility and native Hermes-agent acceptance remain. Not production-ready.** The gateway now exposes only `agents:read-write` to both existing credentials, with unchanged tokens. The dedicated Headless client connected, reported fully synced, and reported upload completion for a create-only setup note. Both credentials and the existing remote helper read the new vault successfully; retired `test`/`test2` IDs are denied and their sync owners are stopped, while local data is retained. Existing HTTPS/Funnel routes are unchanged. The sync container has an `unless-stopped` restart policy; the gateway remains session-managed, not boot-supervised. M0's historical default-merge loss and external-writer CAS limits still apply. See [docs/M0-FINDINGS.md](docs/M0-FINDINGS.md), [docs/TAILNET.md](docs/TAILNET.md), and [docs/M2-FINDINGS.md](docs/M2-FINDINGS.md).

## M0 — Headless feasibility (must complete first)
- [x] Inspect the deployment host directly (or via SSH when remote): OS, architecture, Docker/Compose, disk, and existing proxy/network setup.
- [x] Confirm current official Obsidian Headless installation requirements and terms.
  - Reviewed official installation requirements and legal pages for owner-operated private use; no bundled-client redistribution or hosted-service permission is assumed.
- [x] Build pinned Node/Headless Docker image with persistent credential and vault volumes.
  - Credential home uses Docker volume `lilvault-m0-auth`; vault uses a separate persistent bind mount. Both are writable by UID 1000.
- [x] Authenticate interactively using supported CLI commands.
  - Operator completed login; a fresh container successfully enumerated the approved remote vault without reauthentication.
- [x] Configure a disposable Obsidian Sync vault and verify initial pull.
  - E2EE setup and an empty-vault initial sync passed; a subsequent real desktop edit was downloaded and verified.
- [x] Edit file locally and verify it appears in Obsidian.
  - Live external create detected and uploaded; operator confirmed desktop visibility by editing the note.
- [x] Edit in Obsidian and verify it appears locally.
  - Verified `Obsidian reply: hello from Obsidian` in the local test note.
- [x] Restart container; verify credentials and sync resume.
  - Stopped the original container and recreated it with the same persistent mounts; it reached `Fully synced` without prompts and retained the verified desktop reply.
- [x] Configure second vault with independent local directory/process.
  - `test2` setup succeeded after an initial HTTP 504. Both distinct registrations and concurrent sync processes are verified; second process reached `Fully synced` without prompts. Same-named marker notes with distinct contents uploaded successfully through both processes; separate running mounts verified. Desktop isolation and second-vault return edit passed: the expected reply arrived only in `test2`, while `test` retained its marker and unchanged reply.
- [x] Test external writes, conflicting edits, and offline behavior; document findings.
  - External create/atomic update, conflict outcome, and offline startup/recreation recovery are recorded. Default merge kept the desktop line, not the conflicting local line. Live same-process network-drop recovery remains a hardening follow-up.
- [x] Record exact package version, commands, storage paths, and M0 go/no-go decision.

## M1 — Rust filesystem and REST
- [x] Scaffold Cargo workspace, configuration, logging, and Axum service.
  - One Cargo package with separate modules, as permitted by the spec; split crates only if the vertical slice needs it.
- [x] Implement vault registry and per-agent bearer auth.
- [x] Implement secure path handling and Markdown-only file operations.
- [x] Implement ETag revision checks and serialized gateway writes.
- [x] Implement health, vault list, directory list, read, and write REST routes.
- [x] Add path traversal, ACL, precondition, and race tests.
  - 18 tests passed; `cargo fmt --check`, Clippy with warnings denied, and a real temporary loopback HTTP/SIGTERM smoke test passed. See [docs/M1-FINDINGS.md](docs/M1-FINDINGS.md).

## M2 — MCP vertical slice
- [x] Verify supported `rmcp` Streamable HTTP server API.
  - Official `rmcp` 3.9 stateless server API and typed client verified; declared Rust minimum raised to 1.88.
- [x] Implement authenticated MCP endpoint.
  - Every HTTP request authenticates; no legacy sessions; SDK Host checks and deny-present-Origin policy tested.
- [x] Implement list_vaults, list_files, read_file, write_file tools.
  - Shared core, structured errors, and preconditions tested with a real SDK client over HTTP. All 25 tests, fmt, Clippy, and a production binary build passed. See [docs/M2-FINDINGS.md](docs/M2-FINDINGS.md).
- [x] Add a portable standalone MCP CLI based on the operator-provided Hermes helper.
  - `scripts/lilvault` preserves `tools`/`call`, uses explicit environment/token-file configuration, and passes 10 standard-library HTTP tests plus the existing 25 Rust tests. Live HTTPS discovery/read and the installed loopback client were verified without changing notes. See [docs/CLI.md](docs/CLI.md).
- [ ] Connect Hermes remotely through approved HTTPS/private transport.
  - Private HTTPS :9443 and loopback gateway are live; both unchanged credentials see only `agents`, and retired vault IDs are denied. The existing remote helper read the setup note successfully. Native Hermes-agent acceptance remains separate; see [docs/TAILNET.md](docs/TAILNET.md).
- [ ] Demonstrate Hermes-created note appears in Obsidian.

## M3 — Hardening
- [ ] Add delete_file, search_files, and get_vault_status.
- [ ] Add per-vault sync supervision, degraded state, and useful logs.
- [ ] Test multiple vaults and read-only agents.
- [ ] Test container restart and offline behavior.
  - M0 covered recreation and offline startup; add same-process live reconnect and production Compose/boot-restart verification.
- [ ] Document consistency limitations and conflict behavior.
  - M0 default-merge loss documented; test supported `conflict` strategy before promising conflict preservation.

## M4 — Release packaging
- [ ] Finalize Dockerfiles, Compose, .env/config examples.
- [ ] Add provisioning and backup/recovery instructions.
- [ ] Add README quickstart and operational troubleshooting.
- [ ] Verify clean install on a disposable directory.
- [ ] Run fmt, clippy, unit/integration tests and record results.

## Open questions (resolve during M0)
- [x] Supported Headless authentication mechanism and credentials path.
  - Interactive `login`; observed account token and per-vault configuration beneath `/home/node/.config/obsidian-headless/`, persisted in the sync-only home volume.
- [x] Actual external filesystem change detection semantics.
  - Observed a new Markdown file written externally after continuous sync readiness being detected and uploaded; modify/delete behavior and conflicts still need testing.
- [x] Reliable sync health and conflict observability.
  - Supported `sync-status --json` reports configuration only. Observe connection/upload/conflict logs; unsupported freshness fields must remain unknown and liveness is not readiness.
- [x] Preferred approved HTTPS/tunnel access to the deployment host.
  - Separate Tailnet-only Serve HTTPS :9443 to loopback :8737; never reuse the existing public :443 Funnel.
- [x] Exact vault names/IDs and which disposable vault to use first.
  - `test` and `test2`, exact IDs and isolated storage paths recorded in M0 findings.
