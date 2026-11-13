# TrueNAS / Portainer deployment

**Normal installation: create a dataset in TrueNAS, paste the Compose stack into Portainer, set private variables, and deploy GHCR images.** No SSH directory preparation, NAS builds, or image archives are needed. Obsidian authentication remains a one-time operation; existing credential state can instead be migrated during an approved cutover.

## Deploy

1. Create a dedicated dataset in the TrueNAS UI. Do not point the stack at a shared pool/apps root. The dataset must exist; Compose does not create ZFS datasets.
2. Create a **Docker Standalone** stack in Portainer. Paste [deploy/compose.yaml](../deploy/compose.yaml).
3. Set the following in Portainer's private environment UI:

   | Variable | Purpose |
   |---|---|
   | `LILVAULT_ROOT` | Absolute mountpoint of the dedicated dataset. |
   | `LILVAULT_HERMES_TOKEN` | Required, strong agent bearer token; preserve its current value during migration. |
   | `LILVAULT_MCP_HOST` | Approved private HTTPS authority, e.g. `vault.example:9443`; no scheme/path. Omit for loopback-only MCP. |
   | `LILVAULT_SECOND_AGENT_TOKEN` | Optional second credential, also scoped to `agents:read-write`. |
   | `LILVAULT_IMAGE_TAG` | Defaults to `latest`; use a published release or `sha-<full-commit>` tag for controlled updates. |
   | `LILVAULT_UID` / `LILVAULT_GID` | Default **568:568**, the TrueNAS `apps` user/group; override on other hosts. Never use zero. |

4. Deploy after the [Containers workflow](../.github/workflows/containers.yml) has successfully published the selected tag. Portainer pulls `ghcr.io/lilco-dev/lilvault:<tag>` and `ghcr.io/lilco-dev/lilvault-sync:<tag>`.

[deploy/stack.env.example](../deploy/stack.env.example) lists placeholders only. Keep real credentials out of Git, screenshots, chat, and command arguments. Portainer administrators can inspect container environment values and backups; treat that access as privileged.

Current support: Linux/amd64, kernel **5.6+** (`openat2`), Docker Engine with Compose v2 / Portainer Docker Standalone. Swarm is not supported by this stack's initialization dependencies. Do not put nested mounts/datasets inside a configured vault; secure file operations reject mount-boundary crossings beneath the vault root.

## Automatic initialization

The one-shot `initialize` service:

- Has no network or Docker socket, a read-only image filesystem, and only `CHOWN`, `FOWNER`, and `DAC_OVERRIDE` capabilities.
- Runs briefly as root to prepare **only the mounted stack dataset and its Docker runtime-cache volume**. This is necessary when a newly created dataset is root-owned; a non-root process cannot grant itself access.
- Creates `config`, `vaults/agents`, and `sync-auth` as the configured app UID/GID, mode `0700`.
- Atomically generates `config/gateway.generated.toml`, mode `0600`, from non-secret settings. Tokens are not passed into the initializer or written into this file.
- Rejects unsafe IDs, symlink targets, malformed host input, and unmanaged files at the generated-config path. It does not recursively chown note trees, alter other datasets, or overwrite separate custom configuration files.

An exit code of **0** for this container is expected. Gateway and Sync wait for successful initialization, then run as **568:568**, with `restart: unless-stopped`, no capabilities, and no privileged mode. On daemon restart they reuse the prepared storage; the initializer does not need to remain running.

```text
<stack-root>/
  config/gateway.generated.toml
  vaults/agents/
  sync-auth/
```

`gateway.generated.toml` is managed: change stack variables and redeploy rather than editing it. Advanced custom configurations can override the gateway command to select another private TOML file; the initializer leaves other files alone. An imported working tree/auth home must have app-readable ownership; bootstrap intentionally does not recursively rewrite existing files or guess a migration strategy.

Only Sync mounts `sync-auth`; the gateway mounts just vault data and the read-only configuration directory. The one-shot initializer temporarily sees the dataset to prepare directory metadata. Neither running application has the whole dataset mounted.

## Official Headless installation and login

The official npm package is marked **`UNLICENSED`**. Public availability is not redistribution permission. Consequently our public Sync image contains an open runner, **not a redistributed copy of Obsidian Headless**. On first start the runner installs pinned `obsidian-headless` **0.0.14** from the official npm registry, running as the app user on Node **26.13.0**. Obsidian's [terms](https://obsidian.md/terms) and [license overview](https://obsidian.md/license) apply; an active Sync subscription is required.

Downloaded software lives in a Docker-managed `headless-runtime` volume, separate from notes and credentials. Installation is locked and version/platform-specific; a completion marker is written only after successful installation. Restarts reuse it without npm access, and interrupted installs are retried. No credentials or downloaded proprietary packages are baked into GHCR images. First bootstrap needs outbound npm access; actual Sync needs its normal network access.

## Activate Obsidian Sync (one-time, required)

The gateway will not return notes until this is done. Until then the Sync container stays running, logs **`Waiting for setup`**, and serves nothing but its own console.

**What the operator must do:**

1. Wait for the `initialize` container to exit **0**, then open the **Sync** container console in Portainer. If it offers a choice, choose **`/bin/sh`**; it must be the app user (`568:568`), not root.
2. In that console run `ob login`. Enter the Obsidian account email/password and any 2FA code **at the prompts**.
3. Run `ob sync-list-remote` and note the exact name of the vault you want. The stack expects it to be `agents`.
4. Register it: `ob sync-setup --vault agents --path /vaults/agents` (substitute the real vault name from step 3 if it differs). Enter the vault **encryption password** at the prompt.
5. Exit the console. The runner detects the registration within ~10 seconds and starts continuous sync; logs move past `Waiting for setup`.
6. Confirm the round trip: create a note with the CLI, wait for Headless upload logs, and check it appears in a desktop Obsidian client.

```sh
ob login                          # account email/password + 2FA at prompts
ob sync-list-remote               # exact remote vault name
ob sync-setup --vault agents --path /vaults/agents   # encryption password at prompt
```

Never pass passwords as command-line arguments, and never paste them into Git, chat, screenshots, or logs. Everything here is interactive by design; the credentials land only in `sync-auth`, which the gateway cannot see. Do not start a second continuous sync process against `/vaults/agents`.

If the vault name differs from `agents`, either register it under that path with its real name (`--vault <real-name> --path /vaults/agents`) or change the expected path in the stack before first login. Re-authentication later repeats steps 1–5; it is needed only if the credential home is lost or the session is revoked.

Alternatively, during a controlled migration copy the existing credential home and working tree with app ownership, preserving `/vaults/agents` as the Headless registration path. Preserve existing bearer values in Portainer. Gateway readiness is **local availability**, not cloud acknowledgment; wait for initial pull and an observed desktop round trip before switching agents.

## Publishing and updates

[Containers](../.github/workflows/containers.yml) verifies Rust at the declared minimum toolchain, CLI/Compose checks, automatic initialization, and disposable real-container tests before publishing either image. PRs test but do not publish. Actions use pinned commits; only publishing jobs receive `packages:write`, using the repository's `GITHUB_TOKEN`, not a registry PAT.

- Successful `main` builds publish `latest` and `sha-<full-commit>`.
- A pushed `vX.Y.Z` tag publishes `X.Y.Z`, `X.Y`, and its SHA tag.
- `latest` is mutable; published SHA tags or image digests are preferable for controlled upgrades.
- **Visibility:** both packages were switched to Public after first publication; anonymous pulls were verified for `latest` at digests `sha256:ea19d149…` (gateway) and `sha256:742e2e42…` (sync), `linux/amd64`. Future first-published packages (e.g., after a rename) repeat the one-time visibility step; do not ask NAS users for a GitHub token as a workaround.
- Redeploy the full stack with image pulls enabled after publication. Do not enable Git auto-redeployment that races ahead of the corresponding image builds.

Container restart policies recover process exits and daemon restarts; **unhealthy status alone does not make Docker restart a container**. The gateway health check validates `/readyz`, not cloud freshness. A running Sync process or bootstrap waiter is not proof of successful synchronization.

## Private routing, migration, and recovery

Only `127.0.0.1:8737` is published. Do not expose MCP on a public NAS domain by default. Confirm the existing private proxy/Tailscale network namespace and obtain approval for a private HTTPS route before changing bindings, networks, or another service's route. A proxy container's loopback may not be the NAS host's loopback.

For migration, quiesce old gateway writes and Sync, take a consistent private backup, transfer through a trusted encrypted channel with app ownership, and start the new stack. Test local readiness, both credentials if enabled, `agents`-only scope, conditional writes, and a real cloud/desktop round trip. Verify remote-agent access before switching clients, then explicitly retire the old writable instance. Never run two sync owners against the same shared directory.

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

The opt-in image tests use labeled disposable volumes, fake credentials, and a nondefault app UID. They verify automatic/idempotent initialization, refusal of unsafe settings/symlinks, MCP writes/readiness, an unbundled public runner, real official-package installation, offline cache reuse, and console availability before login. They never use real vaults or production credentials. NAS deployment, actual cloud synchronization, boot acceptance, and backup recovery remain separate checks.
