# lilvault — Implementation Tasks

## Current status

M0 has **verified a real desktop/zoro round trip for remote vault `test`**: the operator saw the uploaded note and the requested desktop reply arrived on zoro. Container recreation also passed without prompts and retained the desktop reply. Offline startup correctly reported a connection failure without a false sync acknowledgment; recreation with normal networking uploaded the queued probe. Both vaults now have independent live sync processes; desktop multi-vault isolation, conflict, and terms checks still gate M1. Live network-drop/automatic reconnection in the same process remains untested. See [docs/M0-FINDINGS.md](docs/M0-FINDINGS.md). GitHub publication still awaits a repository visibility decision.

## M0 — Headless feasibility (must complete first)
- [x] Inspect zoro directly (or via SSH when remote): OS, architecture, Docker/Compose, disk, and existing proxy/network setup.
- [ ] Confirm current official Obsidian Headless installation requirements and terms.
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
  - `test2` setup succeeded after an initial HTTP 504. Both distinct registrations and concurrent sync processes are verified; second process reached `Fully synced` without prompts. Same-named marker notes with distinct contents uploaded successfully through both processes; separate running mounts verified. Desktop isolation and second-vault return edit pending.
- [ ] Test external writes, conflicting edits, and offline behavior; document findings.
  - Live external create/upload passed. Offline startup failed visibly without `Fully synced`; a new normal-network container uploaded a note queued while sync was stopped. Conflict and live network-drop recovery tests remain.
- [ ] Record exact package version, commands, storage paths, and M0 go/no-go decision.

## M1 — Rust filesystem and REST
- [ ] Scaffold Cargo workspace, configuration, logging, and Axum service.
- [ ] Implement vault registry and per-agent bearer auth.
- [ ] Implement secure path handling and Markdown-only file operations.
- [ ] Implement ETag revision checks and serialized gateway writes.
- [ ] Implement health, vault list, directory list, read, and write REST routes.
- [ ] Add path traversal, ACL, precondition, and race tests.

## M2 — MCP vertical slice
- [ ] Verify supported `rmcp` Streamable HTTP server API.
- [ ] Implement authenticated MCP endpoint.
- [ ] Implement list_vaults, list_files, read_file, write_file tools.
- [ ] Connect Hermes remotely through approved HTTPS/private transport.
- [ ] Demonstrate Hermes-created note appears in Obsidian.

## M3 — Hardening
- [ ] Add delete_file, search_files, and get_vault_status.
- [ ] Add per-vault sync supervision, degraded state, and useful logs.
- [ ] Test multiple vaults and read-only agents.
- [ ] Test container restart and offline behavior.
- [ ] Document consistency limitations and conflict behavior.

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
- [ ] Reliable sync health and conflict observability.
- [ ] Preferred approved HTTPS/tunnel access to zoro.
- [ ] Exact vault names/IDs and which disposable vault to use first.
