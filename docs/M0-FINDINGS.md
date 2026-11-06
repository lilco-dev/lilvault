# M0 — Headless feasibility log

## Decision

**Real desktop/zoro round trip verified for remote vault `test` / M0 not passed.** Unattended sync passed after E2EE setup; the operator's requested desktop edit arrived locally. Container recreation also passed without prompts and retained the note; remaining M0 tests still gate M1. Do not start M1 or M2. Work is running directly on zoro; SSH is unnecessary. The operator rebooted the host, resolving the kernel/module mismatch. Normal bridge-network image build and offline CLI checks now pass. No host packages or networking settings were changed by the agent. Sync authentication and round-trip tests remain outstanding.

## Initial investigation — 2026-11-06

### Repository

- The local repository started on `main` with no commits and no remote.
- The supplied `SPEC.md`, `TASKS.md`, and `AGENT_PROMPT.md` were read in full.
- GitHub CLI is authenticated, but `gh repo view lilco-dev/lilvault --json nameWithOwner,visibility,url` could not resolve the repository. This does not distinguish a nonexistent repository from one the current account cannot access.
- Confirm repository visibility before creating or publishing `lilco-dev/lilvault`.

### Direct zoro host inspection

The operator confirmed this session is already on zoro. An initial unnecessary SSH attempt failed; this is **not a project blocker**. `/proc/sys/kernel/hostname` confirms `zoro` (the `hostname` command itself is not installed).

Read-only local inspection found:

- Arch Linux, kernel `7.2.11-arch1-1`, `x86_64`; current user `arch`, UID/GID `1000:1000`.
- Docker CLI `29.9.3`, Docker daemon `29.9.2` with `overlayfs`, Compose `5.7.0`.
- Direct Docker socket access is denied; existing `sudo -n docker` access works. No group or permission changes are needed.
- `sudo -n docker ps` showed no running containers.
- Root filesystem: 50 GiB total, approximately 26 GiB available.
- Local Node `v26.2.0`, Rust `1.97.1`; `ob` is not installed on the host.
- Tailscale is installed, with an existing HTTPS/Funnel route. Do not reuse or alter it without operator approval; public Funnel exposure is not automatically an approved MCP transport.
- No `~/lilvault`, `/srv/lilvault`, or `/opt/lilvault` deployment directory exists.

Commands included `uname -srm`, `id`, selected `/etc/os-release` fields, `docker --version`, `docker compose version`, `sudo -n docker info`, `sudo -n docker ps`, `df -h`, `ss -ltn`, and `tailscale serve status`. No service configuration or secret values were read.

### Official Headless research

Sources inspected:

- [Official repository README](https://github.com/obsidianmd/obsidian-headless/blob/master/README.md)
- [Official package metadata](https://github.com/obsidianmd/obsidian-headless/blob/master/package.json)
- [Obsidian Headless help](https://obsidian.md/help/headless)
- [Headless Sync help](https://obsidian.md/help/sync/headless)
- [Obsidian terms](https://obsidian.md/terms)

Registry query:

```sh
npm view obsidian-headless version engines license dist.integrity --json
```

Observed metadata:

- Package version: `0.0.14`; installed in the M0 image and verified using `ob --version` after reboot.
- Node.js requirement: `>=22.0.0`. Official Node release metadata (`https://nodejs.org/dist/index.json`) lists `v26.13.0` as the latest LTS (Lithium), released 2026-10-28. Image: `node:26.13.0-bookworm-slim`; image build and offline Headless CLI checks passed after reboot.
- Package license metadata: `UNLICENSED`. Do not treat the client as open-source or infer redistribution permission from public availability. Confirm applicable Headless/service terms before publishing an image containing it.
- Obsidian Sync subscription is required.

The official README documents the following command sequence. This is **a proposed interactive procedure, not executed evidence**:

```sh
ob --help
ob login
ob sync-list-remote
# Only inside a newly created, approved disposable vault directory:
ob sync-setup --vault "<approved disposable remote vault>"
ob sync
ob sync --continuous
```

Use interactive prompts for account and encryption credentials, not command-line password arguments or captured logs. Check the installed pinned client's help before automating any command.

The documentation describes continuous filesystem watching, but its actual behavior, exact credential paths, conflict semantics, and unattended authenticated restart remain unverified. No environment-variable authentication mechanism is assumed. The full `/home/node` directory is persisted in a dedicated sync-only volume until actual credential storage is verified.

## Docker build attempt

Executed:

```sh
sudo -n docker build --file deploy/Dockerfile.sync --tag lilvault-sync:m0 .
```

- The pinned `node:26.13.0-bookworm-slim` image pulled successfully; Docker reported digest `sha256:add5c17c14752c54d2f3d519ec46a3815684d41c33f198bcb09fb998f7dcbf68`.
- Docker failed to create a `veth` interface for the build container: `operation not supported`. The `npm install` instruction did not execute successfully, no Headless image was completed, and the planned `ob --version` / `ob --help` checks did not run.
- Read-only diagnosis found running kernel `7.2.11-arch1-1`, but `/usr/lib/modules` contains only `7.2.14-arch1-1`. The running kernel's module tree is absent. `bridge`, `nf_nat`, and `overlay` are loaded; `veth` is not listed in `/proc/modules`. This is consistent with missing modules after a kernel upgrade, but no repair has been attempted.
- Docker also warned that the legacy builder is deprecated and Buildx is absent. This warning was not the cause of failure; no host packages were installed.

A planned reboot into the installed kernel may restore module availability, but requires operator approval and consideration of other services. An explicitly approved host-network build could bypass bridge creation for this test, but would not fix normal bridge-network Compose deployment. Neither workaround has been attempted.

## Post-reboot verification — 2026-11-06

The operator rebooted zoro. Running kernel is now `7.2.14-arch1-1`, its module tree exists, and the Docker daemon reports `29.9.3`. The same build passed without a host-network workaround:

```sh
sudo -n docker build --file deploy/Dockerfile.sync --tag lilvault-sync:m0 .
sudo -n docker run --rm --network none lilvault-sync:m0 --version
sudo -n docker run --rm --network none lilvault-sync:m0 --help
sudo -n docker run --rm --network none lilvault-sync:m0 sync-setup --help
sudo -n docker run --rm --network none lilvault-sync:m0 sync --help
sudo -n docker run --rm --network none lilvault-sync:m0 login --help
sudo -n docker run --rm --network none lilvault-sync:m0 sync-status --help
```

Observed:

- Image: `lilvault-sync:m0`, ID `sha256:bd9f1e19db43cdc0cd8d3af156fbbf30888b10083657d1b615fe2680e2136158`.
- Headless version: `0.0.14`; image user: `node`, working directory: `/home/node`.
- `sync-setup` supports `--vault`, `--path`, and an interactive encryption-password prompt when the password argument is omitted.
- `sync` supports `--path` and `--continuous`; `sync-status` exists. Its actual health semantics are not yet tested.
- Created dedicated Docker volume `lilvault-m0-auth`, mounted at `/home/node`; offline runtime check confirmed UID/GID `1000:1000`, `HOME=/home/node`, and write access. No credentials have been entered. Never mount this home volume into the gateway.

### Next operator action: interactive login

Run in a normal terminal on zoro, not through an agent tool or recorded session:

```sh
sudo docker run --rm -it --log-driver=none \
  --mount type=volume,source=lilvault-m0-auth,target=/home/node \
  lilvault-sync:m0 login
```

Enter account credentials/2FA through the official prompts. Do not paste passwords, authentication tokens, encryption passwords, or credential-file contents into chat. The container is removed after login; the dedicated home volume remains. This persists the whole home directory conservatively; actual credential paths still require verification. Subsequent fresh-container login reuse passed as recorded below. No remote vault is selected or synchronized by this login command.

## Login persistence and first vault preparation

The operator completed interactive login and approved the E2EE remote vault named `test`. A fresh container ran `sync-list-remote --json` using only the persistent home volume. It returned exactly one vault named `test`; unrelated vault names and raw account output were not printed. This verifies reuse of saved authentication, not synchronization or health.

Created `/home/arch/lilvault/data/vaults/test` as a new empty directory with UID/GID `1000:1000` and mode `0700`; existing directories would have caused the operation to fail rather than be reused or overwritten. An offline container verified that this separate bind mount is empty and writable by the non-root runtime. No note files or encryption secrets were written by the agent.

### Next operator action: E2EE vault setup

Run locally and enter the vault encryption password at the official prompt, not as a command-line argument:

```sh
sudo docker run --rm -it --log-driver=none \
  --mount type=volume,source=lilvault-m0-auth,target=/home/node \
  --mount type=bind,source=/home/arch/lilvault/data/vaults/test,target=/vault \
  lilvault-sync:m0 sync-setup \
  --vault test --path /vault --device-name lilvault-zoro-m0
```

This configures only the approved disposable vault. Do not run a concurrent Headless process against the same local path. The operator completed setup; subsequent unattended sync results are recorded below.

## Unattended sync and external-write test

After the operator completed E2EE setup, a fresh noninteractive container ran:

```sh
sudo -n docker run --rm --name lilvault-m0-initial-sync --log-driver=none \
  --mount type=volume,source=lilvault-m0-auth,target=/home/node \
  --mount type=bind,source=/home/arch/lilvault/data/vaults/test,target=/vault \
  lilvault-sync:m0 sync --path /vault
```

It exited `0`, reported a successful connection and `Fully synced`, and did not prompt for account or encryption credentials. Logged settings were bidirectional sync, conflict strategy `merge`, device `lilvault-zoro-m0`, and config syncing disabled. This is the Headless client's reported default merge behavior, not a gateway conflict guarantee. No Markdown notes were downloaded because the remote vault appeared empty.

Filesystem metadata inspection (no credential/config contents read) observed:

- `/home/node/.config/obsidian-headless/auth_token`, mode `0600`.
- `/home/node/.config/obsidian-headless/sync/<remote-vault-id>/config.json`, mode `0600`.
- Per-vault `state.db` and `sync.log` in that same sync-only directory, mode `0644`.
- The local `.obsidian` directory appeared after initial sync.

A second fresh container, named `lilvault-m0-sync`, then started the same command with `--continuous` and also reported `Fully synced` without prompts. It is managed as process `lilvault-m0-test-continuous-sync`. Only this process owns the local test vault.

After that readiness message, an exclusive filesystem create wrote `lilvault-m0-roundtrip.md` at `2026-11-06T04:14:42.321306+00:00`. Existing files were not overwritten. It contains a test heading, the creation timestamp, and `Obsidian reply: pending`. The running Headless process then logged `Uploading file lilvault-m0-roundtrip.md`, `Upload complete lilvault-m0-roundtrip.md`, and `Fully synced`. This verifies detection of a live external filesystem write and the client's reported upload completion. The operator then confirmed completion of the requested desktop edit. The agent verified the exact line `Obsidian reply: hello from Obsidian` in the local note (178 bytes). This confirms desktop visibility and a real Obsidian-to-zoro return edit, not merely a local write or process-liveness assumption.

## Container recreation test

After verifying the desktop reply, the agent stopped only `lilvault-m0-sync` with `docker stop --timeout 15`. Its attached process exited successfully. An immediate recreation using the same name hit a Docker name conflict while automatic removal was still completing; no sync application failure or credential failure occurred. A subsequent inventory confirmed no active containers using this vault. A replacement container named `lilvault-m0-sync-resumed` started at `2026-11-06T04:20:45.084362706Z` with the same persistent mounts and continuous-sync command. It reported `Fully synced` without any interactive prompts. The container was confirmed running, and the original note still contained the exact desktop reply. This verifies persistence and resumption after container recreation; it is not a host-reboot or power-loss test.

For scripts using `--rm`, do not assume `docker stop` has completed name removal before immediately reusing the name; verify removal or use a distinct replacement name after checking the old process is stopped.

## Bounded offline startup and recovery

Stopped only the test sync container, then ran the same client with no container network:

```sh
timeout --signal=TERM --kill-after=5s 20s sudo -n docker run --rm \
  --name lilvault-m0-offline --network none --log-driver=none \
  --mount type=volume,source=lilvault-m0-auth,target=/home/node \
  --mount type=bind,source=/home/arch/lilvault/data/vaults/test,target=/vault \
  lilvault-sync:m0 sync --path /vault
```

Observed `Connecting...`, a server connection error on stderr, and `Disconnected from server`. There was no `Fully synced` message. The process did not exit on its own within 20 seconds; the intentional timeout terminated it with code `124`. The test wrapper treated that deadline as expected, not as successful synchronization. No host network settings were changed.

Assertions verified the error, deadline, absence of a false sync acknowledgment, no remaining active sync owner, and preservation of the original desktop reply. With sync still stopped, an exclusive create wrote `lilvault-m0-offline.md`. This demonstrated local file availability, not cloud acceptance.

Started a fresh normal-network container named `lilvault-m0-sync-recovery`, with the same mounts and `sync --path /vault --continuous`. It reported a successful connection, `Uploading file lilvault-m0-offline.md`, `Upload complete lilvault-m0-offline.md`, then `Fully synced`, without credential prompts. It remains running under process `lilvault-m0-offline-recovery-sync`.

**Scope:** offline startup failure and recovery by container recreation are verified. A live connection loss and automatic reconnection in the same process are not tested. Process liveness alone cannot establish cloud readiness. The recovery probe's desktop visibility has not been independently confirmed; upload completion is the client's reported acknowledgment.

## Operator input required

1. The operator approved remote vault `test2`; a fresh `sync-list-remote --json` lookup found exactly one match. Complete its interactive setup below; do not send account or encryption passwords in chat.
2. Choose private or public visibility for the GitHub repository, and confirm access if it already exists.
3. Choose remote MCP transport later, after existing networking is inspected; no public endpoint is needed to begin sync feasibility tests.

### Second vault preparation and setup

Created new local directory `/home/arch/lilvault/data/vaults/test2`, mode `0700`, UID/GID `1000:1000`, refusing existing paths. Offline checks confirmed an empty, writable non-root mount. The supported `sync-list-local --json` command reported the first vault registered at `/vault`. Because the processes share a credential home, the second vault will use a distinct in-container path `/vaults/test2` rather than reusing `/vault`; this avoids ambiguous local-path registrations. Account/config contents were not read.

Run in a normal terminal and enter any encryption password only at the prompt:

```sh
sudo docker run --rm -it --log-driver=none \
  --mount type=volume,source=lilvault-m0-auth,target=/home/node \
  --mount type=bind,source=/home/arch/lilvault/data/vaults/test2,target=/vaults/test2 \
  lilvault-sync:m0 sync-setup \
  --vault test2 --path /vaults/test2 --device-name lilvault-zoro-m0-test2
```

Second-vault setup completion, saved-key reuse, concurrent independent processes, and isolation remain unverified. The first vault's continuous-sync process is not stopped or reconfigured by this setup command.

## Planned next steps

1. Local read-only host inspection is complete; use existing `sudo -n docker` access for isolated lilvault containers.
2. Report findings and request approval for any missing host-level prerequisites. Keep all work isolated from existing vaults/services.
3. Exact Node/Headless image build, supported CLI help checks, and separate persistent vault mount preparation are complete.
4. Account/config storage paths were observed using filesystem metadata only. Keep the credential home separate from vault data and inaccessible to the future gateway.
5. E2EE setup is complete; unattended saved authentication/decryption worked in both one-shot and continuous fresh containers.
6. Initial sync, local-to-Obsidian visibility, Obsidian-to-local edits, and saved authentication/sync after container recreation are verified.
7. External create/upload and bounded offline startup/recreation recovery are verified. Test independent second-vault sync, conflicts, other external mutations, and live network-drop recovery.
8. Record exact commands, versions, paths, and observed outcomes. Only mark M0 passed after all required real tests succeed.

## Verification so far

- Specification and kickoff instructions reviewed.
- Official documentation and npm metadata inspected.
- Direct zoro host inspection: **passed**. An unnecessary SSH attempt failed but does not block local work.
- Existing passwordless sudo Docker access: **passed**; no permission changes made.
- Initial sync image build failed at Docker bridge setup. After operator reboot: **build passed**, Headless version/help checks passed, dedicated persistent home is writable as non-root.
- Interactive login: **operator completed**. Credential reuse in a fresh container: **passed** via remote vault enumeration.
- Separate persistent vault bind mount: **passed** empty/writable non-root check. E2EE vault setup: **operator completed**.
- Fresh-container unattended one-shot sync: **passed**, exit `0` and `Fully synced`; initial remote was empty.
- Fresh-container continuous sync startup: **passed**, `Fully synced`. Live external test-note detection/upload and human desktop visibility: **passed**.
- Desktop-to-zoro return edit: **passed**; exact expected reply found in the local note.
- Container recreation: **passed**; replacement reached `Fully synced` without prompts and retained the verified desktop reply. A same-name auto-removal race was handled by confirming the old container was gone and using a replacement name.
- Offline startup: **passed within tested scope**; connection error, no `Fully synced`, bounded termination at 20 seconds. Normal-network recreation: **passed**, queued probe uploaded and `Fully synced` reported. Live same-process disconnection/reconnection remains untested.
- Secret/data ignore checks: **passed** (10 ignored paths and 7 publishable exceptions); initial Markdown diagnostics and whitespace checks passed.
- No Rust code exists; Cargo checks are not applicable yet.
- A real Obsidian desktop/zoro round trip is demonstrated. No Rust gateway, Hermes operation, or MCP round trip has been implemented or demonstrated.
