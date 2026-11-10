# Private Tailnet MCP access

## Current deployment scope

The operator approved a separate Tailscale Serve HTTPS listener on **9443**, proxying to lilvault on **`127.0.0.1:8737`**. The existing port-443 Funnel serves another application and was not reused. Before/after checks verified that every existing TCP/Web/Funnel entry remained unchanged and no public Funnel was enabled on 9443. Listener inspection showed loopback only for the gateway and Tailnet interface addresses only for 9443, not wildcard/public binds.

Concrete Tailnet DNS, the generated bearer, runtime configuration, binary snapshot, route snapshots, and verification report are stored privately on the deployment host under `~/lilvault/runtime/`, outside Git. The directory is mode `0700`; config, token, and reports are mode `0600`. Both deployed credentials now have only `agents:read-write`; their token values were preserved during cutover. The gateway has no Obsidian auth volume or Docker socket.

The gateway is running under the session process manager, not a boot-supervised production service. The Serve route is persisted by the existing Tailscale daemon; the dedicated `agents` sync container uses Docker's `unless-stopped` restart policy. A host reboot/session-process stop can still leave an HTTPS proxy whose gateway backend is unavailable. Production Compose/gateway autostart/backup packaging remains M4.

## Verification performed from the deployment host

Using the real HTTPS endpoint, the verifier disabled ambient HTTP proxies and kept normal certificate/hostname verification enabled. It checked:

- Health and local readiness returned 200, with cloud freshness explicitly unknown.
- Unauthenticated MCP initialization returned 401.
- Authenticated SDK initialization succeeded without a session ID.
- Both unchanged credentials exposed exactly `agents` with read-write access; `test` and `test2` operations were denied.
- A create-only setup note was read through the other credential with the same SHA-256 revision, and Headless reported its upload completion.
- Existing port-443 Funnel configuration remained unchanged.

The gateway-side checks establish local service behavior; the remote-helper check below additionally verifies peer reachability and that helper's configured credential. Neither substitutes for confirming the active native Hermes profile or observing the note in desktop Obsidian.

The existing remote helper was subsequently verified over approved read-only SSH access: it discovered only `agents` and read the same setup note/revision using its existing credential. No remote configuration, token source, packages, or services were changed. Native Hermes-agent acceptance and independent desktop Obsidian visibility remain separate checks.

The old `test` and `test2` sync owners are stopped. Their local working trees and the shared authentication volume are retained; no project command deleted remote vaults or local note trees. The new vault was provisioned through the official CLI's supported interactive setup so its encryption password stayed outside chat and logs.

## Remote reachability check (no token needed)

From the Hermes machine, after it is connected to the Tailnet, substitute the deployment host's actual Tailnet DNS:

```sh
curl --noproxy '*' --connect-timeout 10 -i \
  'https://<gateway-tailnet-dns>:9443/mcp'
```

A **401** is the expected unauthenticated result: it proves HTTPS reachability and the bearer boundary without exposing a credential. Do not use `-k` to bypass certificate validation. A connection timeout may indicate an offline peer or Tailnet ACL/port restriction; do not modify ACLs or existing services without approval.

## Hermes configuration shape

The current [official Hermes MCP config reference](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/reference/mcp-config-reference.md) supports URL-based servers and environment references in headers:

```yaml
mcp_servers:
  lilvault:
    url: "https://<gateway-tailnet-dns>:9443/mcp"
    headers:
      Authorization: "Bearer ${LILVAULT_HERMES_TOKEN}"
```

This is a merge fragment, **not** a replacement for an existing profile configuration. Confirm the installed Hermes version and active profile before applying it. `env` on a stdio entry is not a substitute for HTTP headers. Current Hermes resolves this placeholder using the active profile's `.env`/secret source and fails closed if a header secret is missing.

Transfer the generated token from `~/lilvault/runtime/hermes.token` through an existing trusted SSH/file-copy or secret-manager channel. Do not paste it into chat, Git, command arguments, or logs. Provision it as `LILVAULT_HERMES_TOKEN` in the active Hermes profile's private secret source without overwriting unrelated secrets. Preserve existing `mcp_servers` and request approval before changing/restarting a non-lilvault Hermes service. No remote configuration has been applied by this project yet.

## Remaining M2 acceptance

After the actual Hermes client connects:

1. Discover the authorized `agents` vault and read the setup note or another existing Markdown note.
2. Use `write_file` with `create_only: true` to create a new uniquely named disposable note.
3. Verify the actual note/revision on the deployment host and the sync client's upload observation.
4. Have the operator confirm that note appears in desktop Obsidian.

Only then mark the remote Hermes/Obsidian M2 acceptance complete. Local HTTPS or SDK-only success is not a substitute. All [core consistency limits](API.md) and [MCP Host/Origin restrictions](MCP.md) still apply. Do not add a public Funnel or repurpose another application's route to bypass a private connectivity problem.
