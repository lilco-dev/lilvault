# lilvault

A self-hostable gateway that exposes your existing Obsidian Sync vaults to AI agents over MCP (Streamable HTTP), with a small REST API for diagnostics and automation. Agents never need Obsidian installed or a local vault copy.

**Status: feature-complete for v0.1 — all seven MCP tools, Docker deployment, and agent key management are implemented and covered by the test suite. Sync health monitoring and release packaging polish remain before a tagged 1.0.**

## What you get

- **Seven MCP tools**: `list_vaults`, `list_files`, `read_file`, `write_file`, `delete_file`, `search_files`, `get_vault_status` — plus matching REST endpoints under `/v1` for debugging and automation.
- **Your vault, unchanged**: the official Obsidian Headless client syncs bidirectionally with your existing Obsidian Sync subscription. The gateway only reads and writes local files; it never reverse-engineers the sync protocol.
- **Safe by default**: fail-closed configuration, vault-scoped bearer credentials, Linux descriptor-relative path operations (no traversal or symlink escapes), conditional writes and deletes with SHA-256 revision ETags, and size/deadline limits everywhere.
- **ssh-style agent credentials**: one key file per agent (`<name>.token` for read-write, `<name>.ro.token` for read-only), added with a single `lilvault-add-agent` command — no secrets in environment variables or images.
- **Honest consistency**: write success means the local filesystem accepted it, never a claim of cloud synchronization; status reports `unknown` for cloud state the gateway cannot observe.

See the [MCP contract](docs/MCP.md) for the tool reference, the [REST contract](docs/API.md) for routes and filesystem guarantees, and [SPEC.md](SPEC.md) for scope and design decisions.

The intended flow is:

```text
Agent → authenticated MCP over your private HTTPS route
      → Rust gateway → local vault files → official Obsidian Headless
      → Obsidian Sync → your Obsidian clients
```

A successful local write does not imply cloud synchronization. Obsidian credentials remain isolated from the gateway. Note that Headless's default merge can discard a competing edit while reporting success; see the [Headless findings](docs/HEADLESS-FINDINGS.md) before using important notes.

## Run the gateway locally

Requires Linux with `openat2` (kernel 5.6+) and Rust 1.88+. Start with an existing **disposable** local directory, not important notes:

```sh
cp config.example.toml config.toml
mkdir -p keys && openssl rand -hex 32 > keys/my-agent.token
# Edit config.toml: set the vault path and [auth] keys_dir to your absolute paths.
# Keep the loopback listen address unless a secure transport is approved.
cargo run -- config.toml
```

Agents are key files, like ssh `authorized_keys`: `<name>.token` grants read-write on every enabled vault, and `<name>.ro.token` grants read only. Start the server before adding keys if you prefer: an empty key directory starts and denies every request until a key exists. In another terminal:

```sh
curl -H "Authorization: Bearer $(cat keys/my-agent.token)" \
  http://127.0.0.1:8737/v1/vaults
```

`GET /healthz` and `GET /readyz` are unauthenticated probes. The latter measures local roots, **not** cloud sync. Writes require `If-None-Match: *` for creation or an exact `If-Match` ETag for replacement; blind overwrite is rejected. Read [docs/API.md](docs/API.md) for routes, path restrictions, error codes, and concurrency limits.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

## Docker / Compose / Portainer

Create a dedicated directory or dataset, paste [deploy/compose.yaml](deploy/compose.yaml) into Portainer (or run `docker compose up`), set private variables, and pull the published GHCR images. A bounded one-shot initializer creates the layout/config; the long-running services run as a non-root app user and restart automatically. The public Sync image downloads the pinned official Headless client on first start rather than redistributing proprietary software. See [deployment, image tags, and the one-time Obsidian login](docs/PORTAINER.md).

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

## Documentation

- [Deployment guide](docs/PORTAINER.md): Docker/Portainer setup, storage permissions, image tags, agent keys, Obsidian login, upgrades, and recovery.
- [MCP tools](docs/MCP.md): endpoint, authentication, all seven tools, input schemas, results, and limits.
- [REST API](docs/API.md): routes, configuration, filesystem guarantees, revisions, and errors.
- [Command-line client](docs/CLI.md): call MCP tools from a shell without extra packages.
- [Headless findings](docs/HEADLESS-FINDINGS.md): tested behavior of the official Obsidian client, including sync, restart, offline, and conflict observations.
- [Specification](SPEC.md): full scope, security requirements, and design decisions.
- [Development tasks](TASKS.md): maintainer checklist and remaining work.

## Development guardrails

- Use only the official Headless client; do not implement the proprietary Sync protocol.
- Never commit account credentials, bearer tokens, vault content, or sync state. Local data directories and `.env` files are ignored.
- Ask before installing host packages, changing firewall/proxy configuration, or touching existing services.
- Do not expose MCP publicly or assume a hostname or TLS setup.
- The gateway must never mount the sync credential home or Docker socket.
- Back up real vaults; gateway locks do not serialize against external Headless writes.

The intended MCP endpoint is `POST /mcp` on the same listener, authenticated with an agent key's bearer token. The default Host allowlist is loopback-only and browser Origins are denied; behind a reverse proxy, set `LILVAULT_MCP_HOST` to your approved HTTPS authority. Never expose MCP on a public hostname — put it behind a private route you control.
