# NAS / Portainer deployment

This Docker Standalone stack runs the Rust gateway and one official Headless sync process for the `agents` vault. Both use `restart: unless-stopped`. Existing agent tokens can be preserved; no public MCP exposure or unrelated service changes are required.

**Status:** packaging and disposable local-image tests are separate from a real NAS deployment. Do not stop the existing instance or switch clients until the new stack, credentials, storage, and approved private HTTPS route have been verified.

## Requirements

- Linux kernel **5.6+** for secure `openat2` operations. Containers share the NAS kernel.
- Docker Engine and Compose v2 / a Portainer Docker Standalone environment.
- Images built for the NAS architecture. Current image testing targets Linux/amd64; other architectures require validation.
- A dedicated dataset with ownership matching a non-root container UID/GID.
- Existing Obsidian Sync credentials and a provisioned `agents` vault, or supported interactive provisioning with an active subscription.
- An explicitly approved private HTTPS proxy/transport. Having a public NAS web domain does not authorize publishing MCP there.

## Dataset layout and permissions

Choose the absolute dataset mountpoint privately as `LILVAULT_ROOT`. No particular pool, username, or host path is embedded in the stack:

```text
<stack-root>/
  config/gateway.toml
  vaults/agents/
  sync-auth/
```

In an authorized NAS admin terminal, set the correct dataset path and **non-root** numeric UID/GID. The numbers below are examples; use the IDs of your chosen NAS service account, not those of a root admin shell:

```sh
ROOT='/mnt/<pool>/apps/lilvault'
APP_UID=1000
APP_GID=1000
sudo install -d -m 0700 -o "$APP_UID" -g "$APP_GID" \
  "$ROOT" "$ROOT/config" "$ROOT/vaults" \
  "$ROOT/vaults/agents" "$ROOT/sync-auth"
sudo install -m 0600 -o "$APP_UID" -g "$APP_GID" \
  deploy/config.example.toml "$ROOT/config/gateway.toml"
```

Run the last command where the template has been checked out or securely copied. Do not overwrite an existing configuration without reviewing/backing it up. Do not recursively change ownership outside this stack's dataset, grant broad `0777` access, or change other NAS datasets/ACLs. After migration, copied files must also be owned/readable by the selected container user; preserve restrictive permissions.

The gateway mounts only `vaults` and its read-only configuration file. **Only Sync mounts `sync-auth`.** Neither container receives the Docker socket. Missing source paths fail rather than silently becoming root-owned directories.

Use NAS-local storage for the working tree. Avoid placing additional mounts or nested datasets *inside* a configured vault: descriptor-relative operations reject crossing mount boundaries beneath its root. A separate dataset mounted directly as a vault root can be evaluated separately.

## Build and load private images

The Portainer stack deliberately uses `image:` with `pull_policy: never`, not Compose build steps. Portainer documents [limitations for stacks containing build steps on remote environments](https://docs.portainer.io/faqs/known-issues/docker-compose-files-including-build-steps-fail).

From a source checkout on a compatible trusted Docker builder:

```sh
docker build -f deploy/Dockerfile -t lilvault-gateway:0.1.0 .
docker build -f deploy/Dockerfile.sync \
  -t lilvault-sync:0.0.14-node26.13.0 .
```

The build context allows only Rust manifests/source, not config, vaults, credentials, or deployment data. The Sync image installs official `obsidian-headless` 0.0.14 on Node 26.13.0; keep this self-hosted image private and never bake credentials into it. Gateway builds use Rust 1.88.0 and the locked dependency set. No Rust or Python toolchain is needed inside the running gateway image.

If building elsewhere, export these two runtime images, transfer them privately, then load them on the NAS through an authorized admin terminal or an available Portainer image-import workflow:

```sh
docker save -o /secure/path/lilvault-images.tar \
  lilvault-gateway:0.1.0 lilvault-sync:0.0.14-node26.13.0
# On the NAS, with Docker privileges:
docker load -i /secure/path/lilvault-images.tar
```

Do not publish the Sync image or put image/state archives into Git. Do not enable Portainer's forced image-pull option for these local tags. Keep the previous image versions until cutover is accepted.

## Configure and deploy the stack

Review `config/gateway.toml` from [deploy/config.example.toml](../deploy/config.example.toml):

- Listen on **`0.0.0.0:8737` inside the container**. The stack publishes it only on the NAS loopback address.
- Vault ID `agents`, path `/data/vaults/agents`.
- Add the exact approved private HTTPS Host authority to `mcp_allowed_hosts`.
- Preserve existing token values. The example uses `LILVAULT_HERMES_TOKEN`; an optional second agent uses `LILVAULT_SECOND_AGENT_TOKEN`. Map existing private agent records to these environment names without changing their actual bearer values, or explicitly customize the stack environment for additional agents.

In Portainer, create a Docker Standalone stack using [deploy/compose.yaml](../deploy/compose.yaml), via the web editor or repository. Supply these **privately** in the stack's environment UI:

- `LILVAULT_ROOT`: absolute dataset mountpoint.
- `LILVAULT_UID` / `LILVAULT_GID`: the selected non-root ownership IDs.
- `LILVAULT_HERMES_TOKEN`: the existing first agent credential.
- `LILVAULT_SECOND_AGENT_TOKEN`: required only when the second record is enabled.

[deploy/stack.env.example](../deploy/stack.env.example) lists variable names/placeholders, not real secrets. This avoids relying on Portainer being able to read a host-side Compose `env_file`. Portainer administrators can inspect container environment values; treat Portainer access and its backups as privileged. Do not paste credentials into Git, screenshots, chat, command arguments, or logs.

Both services drop capabilities, use a read-only image filesystem, and get writable tmpfs plus only their required persistent mounts. There are no privileged containers or published Sync ports. One Sync process owns `/vaults/agents`; add a separately provisioned process/path per additional vault rather than starting another against this tree.

The gateway image health check calls `/readyz`: it checks **local availability**, not cloud freshness. Docker restart policies recover exited processes and daemon restarts; **unhealthy status alone does not trigger Docker restart**. Sync liveness is not proof of successful cloud acknowledgment.

## Private routing and migration gate

The stack publishes `127.0.0.1:8737` only. A proxy running on the NAS host can forward to it. If Tailscale or the existing proxy runs in a different container/network namespace, its `127.0.0.1` may not be the NAS host. Inspect that arrangement and approve a private network attachment/binding before changing this default. Do not guess an external network name, bind all host interfaces, repurpose another service's route, or enable a public Funnel.

Keep the current instance running during preparation. For the actual cutover:

1. Obtain approval for the new private HTTPS route/client URL and the cutover window.
2. Quiesce gateway writes and the old Sync process, then take a consistent backup of vault data, private gateway config, and the Sync credential home. Never mount the credential home into the gateway.
3. Transfer through a trusted encrypted channel and adjust only stack-file ownership. Preserve the in-container `/vaults/agents` registration path. Alternatively provision a fresh client interactively using supported official commands; never put an encryption password on the command line.
4. Start the NAS stack. Verify local readiness, both existing bearer credentials, `agents`-only scope, and failure for retired vault IDs. Create a uniquely named note with a create-only precondition; read its matching revision, observe Headless upload, and confirm desktop visibility.
5. Verify real access from the agent machine through the approved HTTPS route before updating clients. Keep only one writable gateway deployment active during cutover; never run two sync owners against the same shared working directory.
6. Explicitly retire the old processes after acceptance; retain their data/config for a deliberate rollback window. Do not let old instances auto-start and accept writes unexpectedly.

Snapshots contain plaintext working notes and potentially Sync encryption/authentication material. Protect/encrypt snapshots and replicated backups; snapshots alone are not an off-device backup. **Do not roll back live synced state while Sync is running**: old edits/deletions may propagate. Stop both services and plan reconciliation before restoring a snapshot.

## Verification commands

```sh
python3 -m unittest discover -s tests -p 'test_*.py' -v
# After both images are built, on a trusted Docker builder:
LILVAULT_DOCKER_TESTS=1 \
  python3 -m unittest discover -s tests -p 'test_deploy.py' -v
```

The image tests create labeled, disposable Docker volumes/containers and fake credentials. They check nondefault UID/GID operation, secure filesystem/MCP writes and symlink rejection, readiness, private credential mounts, and the official Headless client's local registry with an isolated private home. They never use real vaults or production credentials. They do not substitute for NAS deployment, actual cloud-sync testing, backup recovery, or boot/restart acceptance.
