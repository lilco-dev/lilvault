# M0 — Headless feasibility log

## Decision

**Awaiting interactive login and disposable vault selection / M0 not passed.** Do not start M1 or M2. Work is running directly on zoro; SSH is unnecessary. The operator rebooted the host, resolving the kernel/module mismatch. Normal bridge-network image build and offline CLI checks now pass. No host packages or networking settings were changed by the agent. Sync authentication and round-trip tests remain outstanding.

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

Enter account credentials/2FA through the official prompts. Do not paste passwords, authentication tokens, encryption passwords, or credential-file contents into chat. The container is removed after login; the dedicated home volume remains. This persists the whole home directory conservatively; actual credential paths and successful reuse in a new container still require verification. No remote vault is selected or synchronized by this login command.

## Operator input required

1. Complete interactive login and select the first disposable Sync vault. Do not send private keys or account passwords in chat.
2. Choose private or public visibility for the GitHub repository, and confirm access if it already exists.
3. A second disposable vault is required for M0 multi-vault acceptance.
4. Choose remote MCP transport later, after existing networking is inspected; no public endpoint is needed to begin sync feasibility tests.

## Planned next steps

1. Local read-only host inspection is complete; use existing `sudo -n docker` access for isolated lilvault containers.
2. Report findings and request approval for any missing host-level prerequisites. Keep all work isolated from existing vaults/services.
3. Exact Node/Headless image build and supported CLI help checks are complete. Next, configure separate vault storage after operator selection.
4. Identify actual account/config storage through supported CLI behavior. Persist credentials separately from vault data; keep gateway access to credentials impossible.
5. Have the operator authenticate interactively and connect the approved disposable vault.
6. Verify initial pull, local-to-Obsidian edits, Obsidian-to-local edits, and authentication/sync after container restart.
7. Verify independent second-vault sync, external writes, conflicts, and offline recovery.
8. Record exact commands, versions, paths, and observed outcomes. Only mark M0 passed after all required real tests succeed.

## Verification so far

- Specification and kickoff instructions reviewed.
- Official documentation and npm metadata inspected.
- Direct zoro host inspection: **passed**. An unnecessary SSH attempt failed but does not block local work.
- Existing passwordless sudo Docker access: **passed**; no permission changes made.
- Initial sync image build failed at Docker bridge setup. After operator reboot: **build passed**, Headless version/help checks passed, dedicated persistent home is writable as non-root.
- Interactive login, credential reuse, vault setup, and actual synchronization: **not yet tested**.
- Secret/data ignore checks: **passed** (10 ignored paths and 7 publishable exceptions); initial Markdown diagnostics and whitespace checks passed.
- No Rust code exists; Cargo checks are not applicable yet.
- No real Obsidian, Hermes, or MCP round trip has been demonstrated.
