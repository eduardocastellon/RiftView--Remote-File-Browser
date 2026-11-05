# RiftView — Remote File Browser (host API)

A self-hosted Rust service that exposes explicitly shared directories over an
HTTP API. See [planning.md](planning.md) for the full specification.

> **Status:** phases 1–3 (foundation, secure filesystem layer, read-only REST
> API). Device auth and TLS are not implemented yet, so the server **refuses to
> bind to anything other than loopback**.

## Run

```bash
cargo run
# overrides
RIFTVIEW_DATA_DIR=/tmp/rv RIFTVIEW_BIND=127.0.0.1:9000 RUST_LOG=debug cargo run
```

Default data directory (holds `config.json` and `shared_directories.json`, owner-only permissions):

| OS      | Path                                      |
|---------|-------------------------------------------|
| macOS   | `~/Library/Application Support/RiftView`  |
| Linux   | `$XDG_DATA_HOME/riftview` or `~/.local/share/riftview` |
| Windows | `%APPDATA%\RiftView`                      |

## API (current)

| Method | Endpoint | Notes |
|---|---|---|
| GET | `/api/health` | `{"status":"ok"}` |
| GET | `/api/version` | |
| GET | `/api/host` | name, os, arch |
| GET/POST | `/api/shares` | POST `{"name","path","writable"?}` |
| GET/PATCH/DELETE | `/api/shares/{id}` | PATCH `{"name"?,"writable"?}` |
| GET | `/api/shares/{id}/files` | `?path=/Docs&sort=name\|size\|modified&order=asc\|desc&offset=&limit=` |
| GET | `/api/files/{id}` | metadata |
| GET | `/api/files/{id}/download` | streaming, single `Range` supported |

```bash
curl -XPOST localhost:7878/api/shares -H 'content-type: application/json' \
  -d '{"name":"Shared","path":"/Users/me/Shared"}'
curl 'localhost:7878/api/shares/<share_id>/files?path=/Documents'
curl -OJ localhost:7878/api/files/<file_id>/download
```

Errors are JSON: `{"error":{"code":"not_found","message":"not found"}}`. They never contain host paths.

## Security model (filesystem)

- Clients address files as `(share id, relative path)`; file ids are an opaque encoding of that pair and are re-validated on every use.
- Paths are parsed strictly (`..`, backslashes, NUL, drive prefixes are rejected, not normalised).
- All access goes through a [`cap-std`](https://docs.rs/cap-std) directory handle on the share root, which confines resolution at the syscall level (`openat2(RESOLVE_BENEATH)` on Linux, handle-based elsewhere). Symlinks that point outside the share are hidden from listings and cannot be opened.
- Shares may not overlap each other or the data directory.
- Files are type-checked on the opened handle (no check-then-open race) and opened non-blocking so FIFOs can't hang a worker.

## Layout

```
src/
  api/         HTTP handlers (thin)
  filesystem/  resolver, roots (shares), browser, metadata, operations — no HTTP
  storage/     atomic JSON state files
  platform/    per-OS data dirs and file permissions
  config.rs  error.rs  state.rs  server.rs
tests/         api_tests.rs, security_tests.rs
```

## Test

```bash
cargo test
```
