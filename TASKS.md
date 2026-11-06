# lilvault — Implementation Tasks

## Current status

M0 is **blocked on Docker bridge networking locally on zoro**; SSH is unnecessary. The pinned Node image pulled, but Docker could not create a `veth` interface; the running kernel and installed module tree differ. No reboot, module installation, or networking workaround has been attempted. See [docs/M0-FINDINGS.md](docs/M0-FINDINGS.md) for evidence and approval options. No sync acceptance tests have run. Disposable Sync vault selection, interactive authentication, and a GitHub repository visibility decision are still required.

## M0 — Headless feasibility (must complete first)
- [x] Inspect zoro directly (or via SSH when remote): OS, architecture, Docker/Compose, disk, and existing proxy/network setup.
- [ ] Confirm current official Obsidian Headless installation requirements and terms.
- [ ] Build pinned Node/Headless Docker image with persistent credential and vault volumes.
- [ ] Authenticate interactively using supported CLI commands.
- [ ] Configure a disposable Obsidian Sync vault and verify initial pull.
- [ ] Edit file locally and verify it appears in Obsidian.
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
- [ ] Supported Headless authentication mechanism and credentials path.
- [ ] Actual external filesystem change detection semantics.
- [ ] Reliable sync health and conflict observability.
- [ ] Preferred approved HTTPS/tunnel access to zoro.
- [ ] Exact vault names/IDs and which disposable vault to use first.
