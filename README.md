# lilvault

A self-hostable gateway that exposes your existing Obsidian Sync vaults to AI agents over MCP (Streamable HTTP), with a small REST API for diagnostics and automation. Agents never need Obsidian installed or a local vault copy.

**Status: feature-complete for v0.1 — all seven MCP tools, Docker deployment, and agent key management are implemented and covered by the test suite. Sync health monitoring and release packaging polish remain before a tagged 1.0.**

## What you get

- **Seven MCP tools**: `list_vaults`, `list_files`, `read_file`, `write_file`, `delete_file`, `search_files`, `get_vault_status` — plus matching REST endpoints under `/v1` for debugging and automation.
- **Your vaults, unchanged**: one Sync container supervises one official Headless process per registered vault and restarts failed children with bounded backoff. Add another vault interactively without editing Compose or restarting the gateway.
- **Official synchronization**: the Headless client syncs bidirectionally with your existing Obsidian Sync subscription. The gateway only reads and writes local files; it never reverse-engineers the sync protocol.
- **Safe by default**: fail-closed configuration, vault-scoped bearer credentials, Linux descriptor-relative path operations (no traversal or symlink escapes), conditional writes and deletes with SHA-256 revision ETags, and size/deadline limits everywhere.
- **Scoped agent credentials**: one token and one explicit vault-permissions file per agent, managed with `lilvault-add-agent` — no secrets in environment variables or images. An agent holding the reserved `auth:manage` permission can also mint and revoke other agents' tokens over HTTP (`POST/GET/DELETE /v1/auth/agents`) — the same surface lilboard exposes at `/api/auth/agents`.
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
printf 'test:read-write\n' > keys/my-agent.permissions
# Edit config.toml: set the vault path and [auth] keys_dir to your absolute paths.
# Keep the loopback listen address unless a secure transport is approved.
cargo run -- config.toml
```

Agents use one `<name>.token` plus a matching `<name>.permissions` file containing explicit `vault:read` or `vault:read-write` lines. An empty key directory starts deny-all. In another terminal:

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

## Add a vault

Open the Sync container console and run:

```sh
ob login                 # once per Obsidian account
lilvault-add-vault       # lists remote vaults and prompts you to choose one
```

The command asks for a local vault ID, invokes the official interactive `sync-setup` password prompt, registers the vault with the gateway, and the built-in supervisor starts its own continuous-sync child within five seconds. Run it again for each additional vault; no YAML edit, extra container, or gateway restart is needed.

## Add or update an agent

In the gateway container, provide explicit vault scopes as arguments or run without them for an interactive prompt:

```sh
docker exec -it <gateway> lilvault-add-agent claude agents:read-write
docker exec -it <gateway> lilvault-add-agent researcher agents:read brain:read
```

For a new agent, the command prompts for one token (`openssl rand -hex 32`). Re-running it for an existing name keeps the token and replaces its permissions. Restart only the gateway container to reload credentials; Sync continues uninterrupted. A legacy read-only `.ro.token` stays read-only; remove that credential and create a new `.token` before granting writes. See [agent keys](docs/PORTAINER.md#manage-agent-keys) and [vault setup](docs/PORTAINER.md#add-and-sync-vaults).

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
