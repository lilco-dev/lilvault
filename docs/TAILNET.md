# Private Tailnet MCP access

## Current deployment scope

The operator approved a separate Tailscale Serve HTTPS listener on **9443**, proxying to lilvault on **`127.0.0.1:8737`**. The existing port-443 Funnel serves another application and was not reused. Before/after checks verified that every existing TCP/Web/Funnel entry remained unchanged and no public Funnel was enabled on 9443. Listener inspection showed loopback only for the gateway and Tailnet interface addresses only for 9443, not wildcard/public binds.

Concrete Tailnet DNS, the generated bearer, runtime configuration, binary snapshot, route snapshots, and verification report are stored privately on zoro under `~/lilvault/runtime/`, outside Git. The directory is mode `0700`; config, token, and reports are mode `0600`. The deployed agent has only `test:read-write`. The gateway has no Obsidian auth volume or Docker socket.

The gateway is running under the session process manager, not a boot-supervised production service. The Serve route is persisted by the existing Tailscale daemon. A host reboot/session-process stop can therefore leave an HTTPS proxy whose backend is unavailable. Production Compose/restart/backup packaging remains M4.

## Verification performed from zoro

Using the real HTTPS endpoint, the verifier disabled ambient HTTP proxies and kept normal certificate/hostname verification enabled. It checked:

- Health and local readiness returned 200, with cloud freshness explicitly unknown.
- Unauthenticated MCP initialization returned 401.
- Authenticated SDK initialization succeeded without a session ID.
- `list_vaults` exposed exactly `test` with read-write access.
- Existing port-443 Funnel configuration remained unchanged.

These tests verify the local end of the route. They do **not** prove that the remote machine's Tailnet ACLs, installed Hermes version, active profile, credentials, and transport are configured correctly.

The operator identified the remote Hermes machine. The initial strict, noninteractive SSH attempt on port 22 timed out; Tailnet status subsequently reported that peer offline. No host-key checks were bypassed, no alternate ports/users were guessed, and no remote files, packages, SSH settings, or Hermes services were changed.

## Remote reachability check (no token needed)

From the Hermes machine, after it is connected to the Tailnet, substitute zoro's actual Tailnet DNS:

```sh
curl --noproxy '*' --connect-timeout 10 -i \
  'https://<zoro-tailnet-dns>:9443/mcp'
```

A **401** is the expected unauthenticated result: it proves HTTPS reachability and the bearer boundary without exposing a credential. Do not use `-k` to bypass certificate validation. A connection timeout may indicate an offline peer or Tailnet ACL/port restriction; do not modify ACLs or existing services without approval.

## Hermes configuration shape

The current [official Hermes MCP config reference](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/reference/mcp-config-reference.md) supports URL-based servers and environment references in headers:

```yaml
mcp_servers:
  lilvault:
    url: "https://<zoro-tailnet-dns>:9443/mcp"
    headers:
      Authorization: "Bearer ${LILVAULT_HERMES_TOKEN}"
```

This is a merge fragment, **not** a replacement for an existing profile configuration. Confirm the installed Hermes version and active profile before applying it. `env` on a stdio entry is not a substitute for HTTP headers. Current Hermes resolves this placeholder using the active profile's `.env`/secret source and fails closed if a header secret is missing.

Transfer the generated token from `~/lilvault/runtime/hermes.token` through an existing trusted SSH/file-copy or secret-manager channel. Do not paste it into chat, Git, command arguments, or logs. Provision it as `LILVAULT_HERMES_TOKEN` in the active Hermes profile's private secret source without overwriting unrelated secrets. Preserve existing `mcp_servers` and request approval before changing/restarting a non-lilvault Hermes service. No remote configuration has been applied by this project yet.

## Remaining M2 acceptance

After the actual Hermes client connects:

1. Discover authorized vaults and read an existing test note.
2. Use `write_file` with `create_only: true` to create a new uniquely named disposable note.
3. Verify the actual note/revision on zoro and the sync client's upload observation.
4. Have the operator confirm that note appears in desktop Obsidian.

Only then mark the remote Hermes/Obsidian M2 acceptance complete. Local HTTPS or SDK-only success is not a substitute. All [core consistency limits](API.md) and [MCP Host/Origin restrictions](MCP.md) still apply. Do not add a public Funnel or repurpose another application's route to bypass a private connectivity problem.
