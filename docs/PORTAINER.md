# TrueNAS / Portainer deployment

**Normal installation: create a dataset in TrueNAS, paste the Compose stack into Portainer, set private variables, and deploy GHCR images.** No SSH directory preparation, NAS builds, or image archives are needed. Obsidian authentication remains a one-time operation; existing credential state can instead be migrated during an approved cutover.

## Deploy

1. Create a dedicated dataset in the TrueNAS UI. Do not point the stack at a shared pool/apps root. The dataset must exist; Compose does not create ZFS datasets.
2. Create a **Docker Standalone** stack in Portainer. Paste [deploy/compose.yaml](../deploy/compose.yaml).
3. Set the following in Portainer's private environment UI:

   | Variable | Purpose |
   |---|---|
   | `LILVAULT_ROOT` | Absolute mountpoint of the dedicated dataset. |
   | `LILVAULT_MCP_HOST` | Approved private HTTPS authority, e.g. `vault.example:9443`; no scheme/path. Omit for loopback-only MCP. |
   | `LILVAULT_IMAGE_TAG` | Defaults to `latest`; use a published release or `sha-<full-commit>` tag for controlled updates. |
   | `LILVAULT_UID` / `LILVAULT_GID` | Default **568:568**, the TrueNAS `apps` user/group; override on other hosts. Never use zero. |

4. Deploy after the [Containers workflow](../.github/workflows/containers.yml) has successfully published the selected tag. Portainer pulls `ghcr.io/lilco-dev/lilvault:<tag>` and `ghcr.io/lilco-dev/lilvault-sync:<tag>`.

Agent credentials are **not** stack variables. They are key files added with `lilvault-add-agent` after the gateway is running; see [Manage agent keys](#manage-agent-keys).

[deploy/stack.env.example](../deploy/stack.env.example) lists placeholders only. Keep real credentials out of Git, screenshots, chat, and command arguments. Portainer administrators can inspect container environment values and backups; treat that access as privileged.

Current support: Linux/amd64, kernel **5.6+** (`openat2`), Docker Engine with Compose v2 / Portainer Docker Standalone. Swarm is not supported by this stack's initialization dependencies. Do not put nested mounts/datasets inside a configured vault; secure file operations reject mount-boundary crossings beneath the vault root.

## Automatic initialization

The one-shot `initialize` service:

- Has no network or Docker socket, a read-only image filesystem, and only `CHOWN`, `FOWNER`, and `DAC_OVERRIDE` capabilities.
- Runs briefly as root to prepare **only the mounted stack dataset and its Docker runtime-cache volume**. This is necessary when a newly created dataset is root-owned; a non-root process cannot grant itself access.
- Creates `config`, `agents`, `registry`, `vaults/agents`, and `sync-auth` as the configured app UID/GID, mode `0700`.
- Atomically generates `config/gateway.generated.toml`, mode `0600`, from non-secret settings. Tokens are not passed into the initializer or written into this file.
- Migrates managed single-vault installs without changing vault contents, `sync-auth`, or existing token files; see **Automatic upgrades** below.
- Rejects unsafe IDs, symlink targets, malformed host input, and unmanaged files at the generated-config path. It does not recursively chown note trees, alter other datasets, or overwrite separate custom configuration files.

**Automatic upgrades:** before changing an existing stack, take a normal private backup and use the updated Compose file so the new registry mounts are present. On redeploy the initializer preserves tokens, notes, and `sync-auth`, rewrites only the managed gateway config to use the registry, creates the existing `agents` marker, and adds an `agents:read-write` or `agents:read` sidecar for each legacy token. Existing permissions files are left untouched. The supervisor imports all valid registrations under `/vaults/<id>` and starts one sync child per vault. No interactive vault setup or Obsidian re-login is needed for an existing working registration.

An exit code of **0** for this container is expected. Gateway and Sync wait for successful initialization, then run as the configured app user (default **568:568**), with `restart: unless-stopped`, no capabilities, and no privileged mode. On daemon restart they reuse the prepared storage; the initializer does not need to remain running.

```text
<stack-root>/
  agents/<name>.token
  agents/<name>.permissions
  config/gateway.generated.toml
  registry/<vault-id>
  vaults/<vault-id>/
  sync-auth/
```

`gateway.generated.toml` is managed: change stack variables and redeploy rather than editing it. Advanced custom configurations can override the gateway command to select another private TOML file; the initializer leaves other files alone. An imported working tree/auth home must have app-readable ownership; bootstrap intentionally does not recursively rewrite existing files or guess a migration strategy.

Only Sync mounts `sync-auth`. Sync mounts the whole `vaults` tree and writes atomic marker files to `registry`; one supervisor launches one official Headless child per registered vault. The gateway mounts vault data and the registry read-only, hot-discovers marker changes on the next request, and mounts `agents` read-write solely for `lilvault-add-agent`. Neither running application has the whole stack dataset mounted.

## Manage agent keys

Each agent has two files:

| File | Contents |
|---|---|
| `<name>.token` | one private 64-hex bearer token |
| `<name>.permissions` | one explicit `vault:read` or `vault:read-write` line per authorized vault |

```sh
openssl rand -hex 32
docker exec -it <gateway> lilvault-add-agent claude agents:read-write
docker exec -it <gateway> lilvault-add-agent researcher agents:read brain:read
# Or omit permissions and choose from the registered-vault prompt:
docker exec -it <gateway> lilvault-add-agent researcher
docker restart <gateway>                    # reload credentials; Sync is untouched
```

For a new name, the command prompts for the token rather than accepting it as an argument. For an existing name it preserves the token and atomically replaces only the permission file, which is how an administrator grants a newly added vault. Removing both files and restarting revokes the credential. Existing unscoped single-vault keys are migrated once to `agents:read` or `agents:read-write` by the initializer; new vaults are never granted automatically.

### Minting over HTTP

An agent holding the reserved `auth:manage` permission (a line in its
`.permissions` file) can mint and revoke other agents' tokens at runtime — no
container restart. The surface is the same shape as lilboard's
`/api/auth/agents`, so one operator agent manages both services with identical
calls:

```sh
MCP_BASE=https://vault.example:9443
ADMIN=$(docker exec <gateway> cat /agents/<manager>.token)

# mint — or rotate by re-posting the same name; the token is shown once
curl -X POST $MCP_BASE/v1/auth/agents -H "Authorization: Bearer $ADMIN" \
  -H "content-type: application/json" \
  -d '{"name": "claude", "permissions": ["agents:read-write"]}'
# 201 {"name":"claude","permissions":["agents:read-write"],"token":"<64hex>"}

# list (names and permissions only, never tokens)
curl $MCP_BASE/v1/auth/agents -H "Authorization: Bearer $ADMIN"

# revoke (204; your own token is refused)
curl -X DELETE $MCP_BASE/v1/auth/agents/claude -H "Authorization: Bearer $ADMIN"
```

Minted keys are written into `agents` as a `<name>.token` + `<name>.permissions`
pair, so they survive restarts and stay visible to `lilvault-add-agent`. Without
an `[auth] keys_dir` (explicit `[[agents]]` entries only), minting returns 409 —
the environment-configured token set has no durable store to write to.

Startup **fails closed** if token/permission entries are symlinks, directories, malformed, oversized, duplicated, or refer to an unregistered vault. An empty key directory is allowed and denies every request. Never paste tokens into chat, Git, screenshots, or command arguments.

## Official Headless installation and login

The official npm package is marked **`UNLICENSED`**. Public availability is not redistribution permission. Consequently our public Sync image contains an open runner, **not a redistributed copy of Obsidian Headless**. On first start the runner installs pinned `obsidian-headless` **0.0.14** from the official npm registry, running as the app user on Node **26.13.0**. Obsidian's [terms](https://obsidian.md/terms) and [license overview](https://obsidian.md/license) apply; an active Sync subscription is required.

Downloaded software lives in a Docker-managed `headless-runtime` volume, separate from notes and credentials. Installation is locked and version/platform-specific; a completion marker is written only after successful installation. Restarts reuse it without npm access, and interrupted installs are retried. No credentials or downloaded proprietary packages are baked into GHCR images. First bootstrap needs outbound npm access; actual Sync needs its normal network access.

## Add and sync vaults

Wait for `initialize` to exit **0**, then open the Sync container console as the app user (`568:568`), not root. Log in once:

```sh
ob login
```

Enter the account password and any 2FA code only at the official prompts. Then run the setup helper with no arguments:

```sh
lilvault-add-vault
```

It displays `ob sync-list-remote`, asks which remote vault to use, suggests a safe local ID, and calls official `ob sync-setup` so the encryption password stays in its interactive prompt. On success it atomically registers `<stack-root>/registry/<id>`. The Sync supervisor detects it within five seconds and launches `ob sync --path /vaults/<id> --continuous`; the gateway hot-discovers the marker without a restart or configuration edit.

For scripting the non-secret choices while retaining the password prompt:

```sh
lilvault-add-vault brain "My Brain Vault"
```

Run the helper again for each vault. Never manually start a second continuous process for a registered path; the supervisor owns one child per vault, restarts failed children with bounded exponential backoff, and terminates all children cleanly with the container. Existing valid Headless registrations under `/vaults/<id>` are imported into the registry automatically on startup.

A new vault grants **no agent access by default**. In the gateway container, update each intended credential, then restart only the gateway:

```sh
lilvault-add-agent claude agents:read-write brain:read-write
```

Finally create a note through MCP, wait for upload logs, and confirm it appears in an Obsidian client. Gateway readiness is local availability, not cloud acknowledgment. Never pass account, 2FA, encryption, or bearer secrets as command-line arguments.

For migration, copy the existing credential home and each working tree with app ownership, preserving registered `/vaults/<id>` paths. The supervisor imports those registrations; recreate agent token and permission files separately.

## Publishing and updates

[Containers](../.github/workflows/containers.yml) verifies Rust at the declared minimum toolchain, CLI/Compose checks, automatic initialization, and disposable real-container tests before publishing either image. PRs test but do not publish. Actions use pinned commits; only publishing jobs receive `packages:write`, using the repository's `GITHUB_TOKEN`, not a registry PAT.

- Successful `main` builds publish `latest` and `sha-<full-commit>`.
- A pushed `vX.Y.Z` tag publishes `X.Y.Z`, `X.Y`, and its SHA tag.
- `latest` is mutable; published SHA tags or image digests are preferable for controlled upgrades.
- **Visibility:** both packages are Public; anonymous pulls for `latest` were verified on `linux/amd64`. Digests change on every republish, so re-verify anonymous access after a release or a package rename instead of trusting a pinned digest; never ask NAS users for a GitHub token as a workaround.
- Redeploy the full stack with image pulls enabled after publication. Do not enable Git auto-redeployment that races ahead of the corresponding image builds.

Container restart policies recover process exits and daemon restarts; **unhealthy status alone does not make Docker restart a container**. The gateway health check validates `/readyz`, not cloud freshness. A running Sync process or bootstrap waiter is not proof of successful synchronization.

## Private routing, migration, and recovery

The stack publishes `8737` on **all host interfaces** (not just loopback) so a containerized reverse proxy can reach the NAS host IP — a proxy container's `127.0.0.1` is not the NAS host. Point the proxy at any NAS address on port 8737, and set `LILVAULT_MCP_HOST` to the proxy's HTTPS authority so MCP accepts that Host. This also means every host on the LAN can reach the gateway directly, bypassing the proxy's TLS; if the LAN is not fully trusted, restrict the publish to a specific IP (e.g. `"192.168.1.10:8737:8737"`) or share a Docker network with the proxy instead of publishing. Do not expose MCP on a public domain, and obtain approval before changing another service's route.

For migration, quiesce old gateway writes and Sync, take a consistent private backup, transfer through a trusted encrypted channel with app ownership, and start the new stack. Test local readiness, every registered vault, each agent's exact scopes, conditional writes, and a real cloud/desktop round trip. Verify remote-agent access before switching clients, then explicitly retire the old writable instance. Never run two sync owners against the same shared directory.

Snapshots can contain plaintext notes and encryption/authentication material. Protect/encrypt backups and keep an off-device copy. Never restore live synced state while Sync is running: old edits/deletions can propagate. Stop both services and plan reconciliation before rollback. The runtime cache is disposable and can be recreated; preserve the dataset and private deployment variables.

## Local verification

Build the two Dockerfiles with the fixture tags used in CI, then:

```sh
LILVAULT_DOCKER_TESTS=1 \
  python3 -m unittest discover -s tests -p 'test_*.py' -v
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

The opt-in image tests use labeled disposable volumes, fake credentials, and a nondefault app UID. They verify automatic/idempotent initialization, refusal of unsafe settings/symlinks, scoped key authentication, `lilvault-add-agent`, interactive vault setup against a fake official CLI, MCP writes/readiness, an unbundled public runner, real official-package installation, offline cache reuse, and console availability before login. A separate supervisor test launches two fake continuous-sync children and removes a stale marker. Tests never use real vaults or deployment credentials; actual cloud synchronization, boot acceptance, and backup recovery remain separate checks.
