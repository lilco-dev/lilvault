# lilvault

A self-hosted gateway exposing existing Obsidian Sync vaults to remote AI agents over MCP Streamable HTTP, with a small REST API for diagnostics and automation.

**Status: M1 REST and the local MCP slice are implemented and tested. Remote Hermes/Obsidian acceptance and production packaging remain. Use disposable vaults only for now.**

## Progress

M0 verified official Obsidian Headless in Docker on a Linux deployment host: real desktop round trips, two isolated vaults, container recreation, external atomic writes, bounded offline recovery, and observed conflict behavior. M1 adds fail-closed configuration, vault-scoped bearer authentication, Linux descriptor-relative Markdown operations, conditional writes, and a REST API. The local MCP slice adds four authenticated tools through the official Rust SDK's stateless Streamable HTTP transport. See [M0 findings](docs/M0-FINDINGS.md), the [REST contract](docs/API.md), and the [MCP contract](docs/MCP.md).

The intended flow is:

```text
Hermes → authenticated MCP over approved HTTPS/private transport
       → Rust gateway → local vault files → official Obsidian Headless
       → Obsidian Sync → Obsidian client
```

A successful local write will not imply cloud synchronization. Obsidian credentials must remain isolated from the gateway. M0 also found that Headless's default merge can discard one competing edit while reporting success; see the findings log before using real notes.

## Run the gateway locally

Requires Linux with `openat2` (kernel 5.6+) and Rust 1.88+. Start with an existing **disposable** local directory, not important notes:

```sh
cp config.example.toml config.toml
mkdir -p keys && openssl rand -hex 32 > keys/hermes.token
# Edit config.toml: set the vault path and [auth] keys_dir to your absolute paths.
# Keep the loopback listen address unless a secure transport is approved.
cargo run -- config.toml
```

Agents are key files, like ssh `authorized_keys`: `<name>.token` grants read-write on every enabled vault, and `<name>.ro.token` grants read only. Start the server before adding keys if you prefer: an empty key directory starts and denies every request until a key exists. In another terminal:

```sh
curl -H "Authorization: Bearer $(cat keys/hermes.token)" \
  http://127.0.0.1:8737/v1/vaults
```

`GET /healthz` and `GET /readyz` are unauthenticated probes. The latter measures local roots, **not** cloud sync. Writes require `If-None-Match: *` for creation or an exact `If-Match` ETag for replacement; blind overwrite is rejected. Read [docs/API.md](docs/API.md) for routes, path restrictions, error codes, and concurrency limits.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

## Docker / NAS / Portainer

Create a dedicated dataset, paste [deploy/compose.yaml](deploy/compose.yaml) into Portainer, set private variables, and pull the published GHCR images. A bounded one-shot initializer creates the layout/config; the long-running services use TrueNAS's `apps` UID/GID **568:568** by default and restart automatically. The public Sync runner downloads the pinned official client on first start rather than redistributing proprietary software. See [Portainer setup, image tags, and one-time Obsidian login](docs/PORTAINER.md). Preparing the stack does not migrate or stop an existing instance.

## Add an agent

Agent credentials are key files, one per agent, like ssh `authorized_keys`. In the running gateway container:

```sh
docker exec -it <gateway-container> lilvault-add-agent claude            # read-write
docker exec -it <gateway-container> lilvault-add-agent claude --read-only
docker restart <gateway-container>                                      # loads the new key
```

The tool prompts for one token, so generate it somewhere private (`openssl rand -hex 32`) and paste it. The file name is the agent name: `<name>.token` (read-write) or `<name>.ro.token` (read-only). Keys grant access to every enabled vault. Malformed, symlinked, oversized, or duplicate key files stop startup rather than silently denying access; an empty directory starts and denies every request until a key is added. See [docs/PORTAINER.md](docs/PORTAINER.md#manage-agent-keys).

## Activate Obsidian Sync (one-time, required)

The gateway returns no notes until Obsidian login happens, and login cannot be scripted. After the stack is up, the Sync container logs **`Waiting for setup`**; open its console in Portainer and run:

```sh
ob login                                             # account + 2FA at prompts
ob sync-list-remote                                  # exact remote vault name
ob sync-setup --vault agents --path /vaults/agents   # encryption password at prompt
```

That one-time step creates the credentials in `sync-auth`, which only the Sync container sees. Sync then starts automatically. Full steps, including the app-user console requirement and the desktop round-trip check, are in [docs/PORTAINER.md](docs/PORTAINER.md#activate-obsidian-sync-one-time-required).

## Command-line client

Use `./scripts/lilvault tools` or `./scripts/lilvault call <tool> '<json-args>'` for immediate MCP access without waiting for an agent's discovery refresh. Requires Python 3 and a credential supplied through `LILVAULT_TOKEN` or `LILVAULT_TOKEN_FILE`; no extra Python packages. See [CLI usage and tests](docs/CLI.md).

## Project documents

- [SPEC.md](SPEC.md): scope, fixed decisions, security requirements, and acceptance criteria.
- [TASKS.md](TASKS.md): milestone checklist and current blockers.
- [AGENT_PROMPT.md](AGENT_PROMPT.md): kickoff instructions.
- [docs/M0-FINDINGS.md](docs/M0-FINDINGS.md): Headless research and real sync verification.
- [docs/API.md](docs/API.md): REST usage, configuration, filesystem guarantees, and limits.
- [docs/CLI.md](docs/CLI.md): standalone MCP command-line client and credential configuration.
- [docs/M1-FINDINGS.md](docs/M1-FINDINGS.md): REST implementation scope and verification evidence.
- [docs/MCP.md](docs/MCP.md): MCP URL, tools, authentication, Host/Origin policy, and limits.
- [docs/M2-FINDINGS.md](docs/M2-FINDINGS.md): local SDK-client verification and remote demo prerequisites.
- [docs/TAILNET.md](docs/TAILNET.md): approved private HTTPS route, credential handling, and pending Hermes setup.
- [docs/PORTAINER.md](docs/PORTAINER.md): Docker images, dataset permissions, Portainer variables, and migration gates.

## Development guardrails

- Use only the official Headless client; do not implement the proprietary Sync protocol.
- Never commit account credentials, bearer tokens, vault content, or sync state. Local data directories and `.env` files are ignored.
- Ask before installing host packages, changing firewall/proxy configuration, or touching existing services.
- Do not expose MCP publicly or assume a hostname or TLS setup.
- The gateway must never mount the sync credential home or Docker socket.
- Back up real vaults; gateway locks do not serialize against external Headless writes.

MCP is available at `/mcp` on the same listener with the same bearer token. Its default Host allowlist is loopback-only and browser Origins are denied. There is no remote Hermes demo or production Compose gateway deployment yet. The Headless feasibility containers and gateway remain separate; local write success does not establish a cloud round trip.
