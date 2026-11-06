# M0 — Headless feasibility log

## Decision

**BLOCKED on Docker bridge networking / not validated.** Do not start M1 or M2. Work is running directly on zoro; SSH is unnecessary. No host packages, networking settings, or existing services have been changed. Sync authentication and round-trip tests remain outstanding.

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

- Package version: `0.0.14` (candidate for the M0 pin, not yet installed/tested).
- Node.js requirement: `>=22.0.0`. Official Node release metadata (`https://nodejs.org/dist/index.json`) lists `v26.13.0` as the latest LTS (Lithium), released 2026-10-28. Candidate image: `node:26.13.0-bookworm-slim`; availability and CLI compatibility must be verified by building.
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

The documentation describes continuous filesystem watching, but its actual behavior, persistence paths, conflict semantics, and unattended restart remain unverified. No credential path or environment-variable authentication mechanism is assumed.

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

## Operator input required

1. Choose how to address the Docker networking blocker: an operator-planned kernel reboot/repair, or explicit approval to try host networking for the isolated image build only. No reboot or host-network workaround is authorized yet.
2. Select disposable Sync vaults and arrange interactive authentication; no SSH fix is required. Do not send private keys or account passwords in chat.
3. Choose private or public visibility for the GitHub repository, and confirm access if it already exists.
4. A second disposable vault is required for M0 multi-vault acceptance.
5. Choose remote MCP transport later, after existing networking is inspected; no public endpoint is needed to begin sync feasibility tests.

## Planned next steps

1. Local read-only host inspection is complete; use existing `sudo -n docker` access for isolated lilvault containers.
2. Report findings and request approval for any missing host-level prerequisites. Keep all work isolated from existing vaults/services.
3. Select exact Node and Headless versions; build a non-root sync-only Docker image. Verify installed `ob --help` and supported subcommands.
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
- Pinned Node image pull: **passed**. Sync image build: **failed at Docker bridge setup**; Headless CLI checks did not run.
- Secret/data ignore checks: **passed** (10 ignored paths and 7 publishable exceptions); initial Markdown diagnostics and whitespace checks passed.
- No Rust code exists; Cargo checks are not applicable yet.
- No real Obsidian, Hermes, or MCP round trip has been demonstrated.
