# RiftView — Remote File Browser (host API)

A self-hosted Rust service that lets your paired devices browse and download folders you choose to share. It runs on your computer and is reached over your LAN or any VPN you like (Tailscale, Radmin, Hamachi…). No cloud service involved. See [planning.md](planning.md) for the full specification.

> **Status:** phases 1–6 are done: secure filesystem layer, read API, HTTPS, device pairing, login and permissions, the download engine (streaming, resume with `ETag`/`If-Range`, SHA-256 verification, transfer limits) and resumable chunked uploads. File management (rename, move, delete, new folder; phase 7) is next.

## Quick start

```bash
cargo run                                     # start the server (same as `serve`)

# in another terminal
cargo run -- share add Photos ~/Pictures      # share a folder (read-only)
cargo run -- share add Inbox ~/Inbox --writable
cargo run -- share list                       # shares with their IDs
cargo run -- share rename shr_… "Holiday Photos"   # change the name devices see
cargo run -- share set-writable shr_… yes     # allow changes (uploads…); `no` = read-only
cargo run -- share remove shr_…               # stop sharing (the folder isn't touched)
cargo run -- pair                             # shows a 6-digit code; approve the device
cargo run -- devices                          # list paired devices
cargo run -- rename dev_… "Work phone"        # your own name for a device (omit to clear)
cargo run -- revoke dev_…                     # cut a device off immediately
cargo run -- info                             # addresses and certificate pin
```

## How it fits together

| Listener | Default | Who uses it |
|---|---|---|
| Device API, **HTTPS** | `0.0.0.0:7878` (all interfaces) | Paired devices (the React Native app) |
| Admin API, plain HTTP | `127.0.0.1:7879` (this computer only) | The `riftview-api` commands above |

- **HTTPS identity:** a self-signed certificate is created on first run. Devices learn its pin during pairing and refuse any other certificate afterwards.
- **Pairing:** six-digit code shown on the host, entered on the device. The code is never sent over the network (SPAKE2). A wrong guess burns the code. The host user then approves the device as read-only or full access.
- **Login:** every connection, the device signs a single-use challenge with its Ed25519 key and gets a session token (1 hour).
- **Admin API:** loopback only, and it also needs the token in `<data dir>/admin_token`, which only your OS user can read.
- **Storage:** shares, devices, device permissions and unfinished uploads live in a SQLite database, `riftview.db`, in the data folder. Settings and secrets stay as files.
- **Uploads:** partial data is written to a hidden `.riftview-upload-<id>.part` file in the target folder, so finishing is an instant rename on the same disk. Devices never see these files.

Client developers: the exact protocol is in [docs/auth-protocol.md](docs/auth-protocol.md).

## Configuration

`config.json` in the data folder (created on first run):

| Field | Default | |
|---|---|---|
| `bind` | `0.0.0.0:7878` | Device API address. Use a specific IP (e.g. your Tailscale `100.x.y.z`) to listen on one interface only. |
| `admin_bind` | `127.0.0.1:7879` | Must be loopback. |
| `session_lifetime_secs` | `3600` | |
| `auth_rate_limit_per_minute` | `20` | Pairing/login requests per IP. |
| `max_concurrent_transfers` | `16` | Transfers running at once, all devices: downloads, hash requests, upload chunks, finishing uploads. (Old name `max_concurrent_downloads` still accepted.) |
| `max_transfers_per_device` | `4` | The same, per device. (Old name `max_downloads_per_device` still accepted.) |
| `max_upload_bytes` | `10737418240` (10 GiB) | Largest file a device may upload. Uploads also need the file size + 1 GiB free on the disk. |
| `max_uploads_per_device`, `max_uploads_total` | `4`, `32` | Unfinished uploads open at once. |
| `upload_expiry_secs` | `86400` (24 h) | Unfinished uploads with no new chunk for this long are deleted. |
| `chunk_upload_timeout_secs` | `300` | Time allowed to receive one upload chunk. |
| `max_list_limit`, `max_dir_entries`, `request_timeout_secs`, `max_request_body_bytes` | | Resource limits |

Environment overrides: `RIFTVIEW_DATA_DIR`, `RIFTVIEW_BIND`, `RUST_LOG`.

Data folder (owner-only permissions):

| File | Holds |
|---|---|
| `riftview.db` (+ `-wal`, `-shm`) | Shares, devices, device permissions, unfinished uploads (SQLite) |
| `config.json` | Settings above |
| `admin_token` | Secret for the admin API |
| `tls/` | Self-signed certificate and its private key |


| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/RiftView` |
| Linux | `$XDG_DATA_HOME/riftview` or `~/.local/share/riftview` |
| Windows | `%APPDATA%\RiftView` |

## Device API

| Method | Endpoint | Needs |
|---|---|---|
| GET | `/api/health`, `/api/version` | — |
| POST | `/api/pairing/initiate`, `/api/pairing/complete` | — (rate-limited) |
| GET | `/api/pairing/{id}` | — (rate-limited) |
| POST | `/api/auth/challenge`, `/api/auth/session` | — (rate-limited) |
| DELETE | `/api/auth/session` | login |
| GET | `/api/devices/me`, `/api/host` | login |
| GET | `/api/shares`, `/api/shares/{id}` | `browse` |
| GET | `/api/shares/{id}/files?path=&sort=&order=&offset=&limit=` | `browse` |
| GET | `/api/files/{id}` | `browse` |
| GET | `/api/files/{id}/download` (supports `Range` + `If-Range`, sends `ETag`) | `read` |
| GET | `/api/files/{id}/hash` → `{sha256, size, etag}` (no request timeout; cached) | `read` |
| POST | `/api/transfers` (start an upload) | `upload` + writable share |
| GET | `/api/transfers`, `/api/transfers/{id}` (my unfinished uploads, progress) | `upload` |
| PUT | `/api/transfers/{id}/chunks/{index}` (4 MiB chunk, `X-Chunk-SHA256`) | `upload` + writable share |
| POST | `/api/transfers/{id}/complete` (whole-file SHA-256 check, save; renames on a clash) | `upload` + writable share |
| DELETE | `/api/transfers/{id}` (cancel) | `upload` |

Errors are JSON: `{"error":{"code":"not_found","message":"not found"}}`. They never contain host paths.

## Security model (filesystem)

- Devices address files as `(share id, relative path)`. File IDs are an opaque encoding of that pair and are re-checked on every use.
- Paths are parsed strictly: `..`, backslashes, NUL bytes and drive prefixes are rejected, not "cleaned up".
- All access goes through a [`cap-std`](https://docs.rs/cap-std) handle on the share root, which keeps lookups inside the share at the OS level. Symlinks pointing outside a share are hidden and can't be opened.
- Shares may not overlap each other or the data folder. Only the host can create shares.

## Layout

```
src/
  api/         device API handlers (thin)
  admin/       host-only admin API + admin token
  auth/        pairing (SPAKE2), login sessions, devices & permissions, rate limit
  filesystem/  resolver, shares, browsing, metadata, file opening — no HTTP
  transfers/   download streaming, ETags, SHA-256 hashing, transfer limits, uploads + cleanup
  database/    SQLite connection and numbered schema migrations (migrations/*.sql)
  storage/     atomic writes for config, admin token, certificate
  platform/    per-OS data folders and file permissions
  cli.rs       `pair`, `share`, `devices`, … commands
  tls.rs       self-signed certificate and pin
  config.rs  error.rs  state.rs  server.rs  main.rs
tests/         api_tests, auth_tests, security_tests (+ common/ reference client)
docs/          auth-protocol.md
```

## Test

```bash
cargo test
```
