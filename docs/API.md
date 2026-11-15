# REST API and local consistency

Linux with `openat2` support is required (kernel 5.6+); unsupported kernels fail closed. This is a local-filesystem API, not a cloud-sync acknowledgment service. The local [MCP transport](MCP.md) uses this same core.

## Configuration

Copy `config.example.toml` to ignored `config.toml`. Set an **existing, absolute, non-symlink disposable vault directory**, and keep `127.0.0.1:8737` unless approved transport is configured. Explicit `[[vaults]]` entries cannot overlap or alias. The container stack instead uses `[vault_registry]` with a read-only marker directory and vault root; each safe marker `<id>` maps to `<root>/<id>` and is hot-discovered on the next request. Registry symlinks, invalid IDs, missing roots, or more than 1,024 markers fail closed.

Tokens must be 32 cryptographically random bytes encoded as 64 hexadecimal characters. The server validates representation/length, not randomness; use a generator, never a human password. It stores SHA-256 digests for comparison and never logs token values.

Two credential sources exist and cannot be combined. `[auth] keys_dir` loads `<name>.token` (one token) plus `<name>.permissions` (one explicit `vault-id:read` or `vault-id:read-write` line per authorized vault). Legacy `<name>.ro.token` is accepted only with read permissions. Entries load at startup, so credential changes require a gateway restart; an empty directory starts deny-all. Symlinks, directories, missing sidecars, malformed or oversized content, duplicate names/tokens, and unknown vault permissions fail startup. Alternatively, `[[agents]]` entries read tokens from `token_env` variables; those names use uppercase ASCII letters, digits, and underscores, and the server does **not** load `.env` automatically. Vault IDs and agent names use 1–64 ASCII letters/digits/underscore/hyphen. No secret is accepted inline in TOML, and errors intentionally omit TOML excerpts.

## Routes

All `/v1` routes require exactly one `Authorization: Bearer <token>` header. Probes return no vault names or secrets.

| Method and path | Result |
|---|---|
| `GET /healthz` | Process liveness |
| `GET /readyz` | Local roots still match the initialized directories; 503 otherwise. `sync` is explicitly `unknown`. |
| `GET /v1/vaults` | `{"vaults":[{"id":"test","access":"read-write"}]}` for this caller only |
| `GET /v1/vaults/{id}/files?path=folder` | `{"files":[{"path":"folder/note.md","type":"file","size":42}]}`; immediate children only. Omit `path` for root. |
| `GET /v1/vaults/{id}/files/{path}` | Raw UTF-8 Markdown; `Content-Type: text/markdown; charset=utf-8` and strong `ETag` |
| `PUT /v1/vaults/{id}/files/{path}` | Raw UTF-8 body; requires a create-only or matching-revision header |
| `DELETE /v1/vaults/{id}/files/{path}` | Requires an exact `If-Match`; `If-None-Match: *` is invalid for deletion. Returns `{"deleted":true,"cloud_sync":"unknown"}` |
| `GET /v1/vaults/{id}/search?q=needle&limit=20` | Bounded case-insensitive filename/content substring search; `{"matches":[{"path":"...","snippet":"..."}]}`; `limit` 1-100, snippets lowercased, filename-only matches have an empty snippet |
| `GET /v1/vaults/{id}/status` | `{"vault_id","access","local_available","sync":"unknown"}`; the gateway cannot observe the sync process or cloud freshness |

Only visible directories and single-linked `.md` regular files are listed. Symlinks, hard-linked files, hidden entries, non-UTF-8 names, and attachments are omitted. Empty directories are returned. File size is in bytes; directory size is zero. Listing more than 10,000 raw directory entries fails with 413 rather than returning a misleading partial list.

Paths are relative, forward-slash separated, and decoded exactly once by the HTTP framework. Unicode names and encoded spaces are supported. Absolute paths, empty interior/trailing components, dot-prefixed components (including `.obsidian`), traversal, literal percent signs (including double-encoded paths), backslashes, and control/NUL characters are rejected. Non-root paths have a 1,024-byte limit. Note paths must end in lowercase `.md`. Parent directories must already exist. Nested mounts and symbolic links are rejected during descriptor-relative resolution; hard-linked notes are rejected as well. Some otherwise valid Unix filenames are deliberately unsupported for an unambiguous agent contract.

## Writes and revisions

For creation, send `If-None-Match: *`. If the destination exists, the API will not overwrite it. For replacement, use the exact quoted ETag returned by a read:

```http
If-Match: "<64 lowercase SHA-256 hex characters>"
```

The digest covers exact file bytes, not mtime. Blind overwrite (`If-Match: *`), weak ETags, multiple tags, multiple conditional headers, and simultaneous `If-Match`/`If-None-Match` are not supported. Creation returns 201; replacement returns 200. Both include the new ETag and:

```json
{"revision":"<SHA-256 hex>","local_write":true,"cloud_sync":"unknown"}
```

The request body and file reads are limited to 2 MiB, must be valid UTF-8, and are not transformed. Permission checks happen in the shared service, not only the HTTP layer. Read-only credentials receive 403 on otherwise valid writes. Unauthorized and nonexistent vaults are both 404; unauthenticated callers receive 401 before any vault operation.

Filesystem I/O runs off the async executor. Gateway writes serialize per vault, re-check the digest while holding the lock, fsync a hidden temporary file in the same directory, re-check the revision again, atomically rename, then fsync the directory. Create-only commit uses Linux `RENAME_NOREPLACE`. Abandoned temporary files are cleaned up.

**Bounded guarantee:** concurrent gateway updates to the same revision cannot both succeed. Headless and other local writers bypass gateway locks; a change detected before commit is rejected, but an external edit in the final check/rename window may still be replaced. This is not distributed compare-and-swap. The gateway performs no merge. Testing found that Headless's default `merge` can drop a competing edit while reporting success. Back up real vaults and test a conflict-preserving deployment policy before relying on this with important notes.

Roots are opened with symlink-rejecting `openat2`; child operations are anchored to directory descriptors, not check-then-open absolute strings. Replacing/unmounting a configured root makes readiness/operations fail closed. An already-open directory descriptor prevents a symlink swap from redirecting I/O; trusted external processes can still rename or remove directories. This is not protection against a malicious host administrator or a process already allowed to read/write all vault data.

The HTTP deadline is 30 seconds. A timed-out/connection-lost write may still finish in the blocking filesystem worker, and a post-rename fsync failure can leave an accepted write with uncertain durability. Read the current file/revision before retrying. Errors do not imply an automatic rollback.

## Errors

Errors use `{"error":{"code":"...","message":"..."}}`, including extraction, size, not-found, and method errors. Main status codes are 400 invalid input/path, 401 missing/invalid bearer, 403 read-only, 404 unavailable identity/resource, 412 stale/create-existing precondition, 413 size/listing limit, 428 missing precondition, 503 unsafe/unavailable root or I/O failure, and 504 deadline exceeded. `/mcp` is documented separately in [MCP.md](MCP.md). Search is a bounded scan (10,000 entries); larger vaults should page through `list_files` instead. A 409 conflict signal is reserved for later reliable sync/conflict observation; this slice does not claim it can detect every cloud conflict.

## Development checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Tests use temporary vaults and deterministic test-only credentials. Do not copy their token values into a deployment. The gateway needs neither a Docker socket nor the Obsidian credential home.
