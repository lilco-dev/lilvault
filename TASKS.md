# lilvault — Implementation Tasks

## Current status

M0 is **testing live bidirectional sync for the approved remote vault `test`**. E2EE setup completed; fresh-container one-shot sync and continuous startup succeeded without prompts. The remote vault initially contained no Markdown notes, so download verification is still pending. A test note was created externally while continuous sync was running; desktop confirmation and a reply edit are required. See [docs/M0-FINDINGS.md](docs/M0-FINDINGS.md). GitHub publication still awaits a repository visibility decision.

## M0 — Headless feasibility (must complete first)
- [x] Inspect zoro directly (or via SSH when remote): OS, architecture, Docker/Compose, disk, and existing proxy/network setup.
- [ ] Confirm current official Obsidian Headless installation requirements and terms.
- [x] Build pinned Node/Headless Docker image with persistent credential and vault volumes.
  - Credential home uses Docker volume `lilvault-m0-auth`; vault uses a separate persistent bind mount. Both are writable by UID 1000.
- [x] Authenticate interactively using supported CLI commands.
  - Operator completed login; a fresh container successfully enumerated the approved remote vault without reauthentication.
- [ ] Configure a disposable Obsidian Sync vault and verify initial pull.
  - E2EE setup and an empty-vault initial sync passed; a real desktop-to-zoro note/edit still needs verification.
- [ ] Edit file locally and verify it appears in Obsidian.
  - Live external create detected; Headless logged `Upload complete lilvault-m0-roundtrip.md`. Desktop confirmation pending.
- [ ] Edit in Obsidian and verify it appears locally.
- [ ] Restart container; verify credentials and sync resume.
- [ ] Configure second vault with independent local directory/process.
- [ ] Test external writes, conflicting edits, and offline behavior; document findings.
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
