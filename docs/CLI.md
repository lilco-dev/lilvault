# Command-line client

`scripts/lilvault` is a small Python 3 client for the existing MCP endpoint. It is adapted from an operator-provided Hermes helper and requires no additional Python packages. It is useful when an agent can run commands immediately but its native MCP tool discovery will not refresh until the next turn. It does not bypass gateway authentication, ACLs, or revision checks.

## Run or install

Run directly from a checkout:

```sh
./scripts/lilvault --help
```

Optionally install on your PATH (back up any existing command first):

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 scripts/lilvault "$HOME/.local/bin/lilvault"
```

This Python client and the Rust server executable share the name `lilvault`. Installing the client must not overwrite a server executable you rely on; run the server through its explicit binary path or `cargo run -- config.toml`.

## Connection and credential

The default URL is the loopback gateway, `http://127.0.0.1:8737/mcp`. For approved remote access, set the full HTTPS MCP URL:

```sh
export LILVAULT_URL='https://<gateway-tailnet-dns>:9443/mcp'
export LILVAULT_TOKEN_FILE='/path/to/private/token'
```

The token file contains the raw bearer token, not a dotenv assignment. Keep it owner-readable only, preferably mode `0600` inside a `0700` directory, and outside Git. Alternatively use `LILVAULT_TOKEN` from your existing secret source; a nonempty environment token takes precedence over the file. There is no token command-line option and no automatic reading of a personal application's dotenv file.

HTTPS is required except for loopback HTTP. Certificate verification stays enabled. Redirects are refused so credentials are not forwarded elsewhere; ambient HTTP proxies are ignored for local/private traffic. See [TAILNET.md](TAILNET.md) for private transport provisioning.

## Commands

```sh
lilvault tools
lilvault call list_vaults
lilvault call list_files '{"vault_id":"test"}'
lilvault call read_file '{"vault_id":"test","path":"hello.md"}'
lilvault call write_file '{"vault_id":"test","path":"new.md","content":"Hello\n","create_only":true}'
```

For updates, supply the `expected_revision` returned by the last read instead of `create_only`. The CLI forwards these arguments unchanged; it never invents a revision, adds an overwrite mode, or silently retries a conflicting write.

For larger or private arguments, use `-` to read the JSON object from stdin rather than putting note contents into command arguments or shell history:

```sh
lilvault call write_file - < private-request.json
```

Keep private request files outside Git too. Tool output is printed as text or JSON without the JSON-RPC envelope. Exit status is zero on success and nonzero on usage, configuration, authentication, HTTP/protocol, or MCP tool errors. Help does not require a token or a running gateway.

## Transport and consistency limits

Each invocation initializes MCP, sends the initialized notification, and calls the requested tool with bearer authentication on every request. It negotiates protocol `2025-06-18`, carries a returned session ID when present, and handles JSON and buffered POST SSE responses. This is a focused lilvault client, not an SDK replacement for persistent notification streams or session resumption.

Local write success still does **not** acknowledge Obsidian Sync. All [core consistency limits](API.md) and [MCP preconditions](MCP.md) apply. A real desktop round trip must be observed separately.

## Tests

```sh
python3 -m unittest discover -s tests -p 'test_cli.py' -v
```

The tests use an ephemeral loopback HTTP fixture and fake credentials, not real vaults. They cover the initialization/notification sequence, per-request auth, session/protocol headers, stdin and token-file input, unchanged create/revision arguments, JSON/SSE results, failures, redirect refusal, proxy isolation, and help without credentials.
