# lilvault — Implementation Tasks

## Current status

**M1 and the local M2 MCP slice are implemented and verified; remote Hermes acceptance awaits approved transport. Not production-ready.** Real desktop round trips, multi-vault isolation, container recreation, external create/atomic-update detection, and bounded offline failure/recovery are verified. Default Headless `merge` discarded the competing local line in the conflict probe despite reporting success; document this limit and test alternate conflict preservation before real-vault rollout. Both sync processes are running. Live same-process reconnection and boot autostart remain hardening work. See [docs/M0-FINDINGS.md](docs/M0-FINDINGS.md). The operator approved public GitHub publication to `lilco-dev/lilvault`; remote transport still requires an operator decision.

## M0 — Headless feasibility (must complete first)
- [x] Inspect zoro directly (or via SSH when remote): OS, architecture, Docker/Compose, disk, and existing proxy/network setup.
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
- [ ] Connect Hermes remotely through approved HTTPS/private transport.
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
- [ ] Preferred approved HTTPS/tunnel access to zoro.
- [x] Exact vault names/IDs and which disposable vault to use first.
  - `test` and `test2`, exact IDs and isolated storage paths recorded in M0 findings.
