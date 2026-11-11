# RiftView — Remote File Browser (host API)

A self-hosted Rust service that lets your paired devices browse and download folders you choose to share. It runs on your computer and is reached over your LAN or any VPN you like (Tailscale, Radmin, Hamachi…). No cloud service involved. See [planning.md](planning.md) for the full specification.

> **Status:** phases 1–4 and the download part of phase 5 are done: secure filesystem layer, read API with streaming downloads, HTTPS, device pairing, login and permissions. Uploads and file management (phases 6–7) are next.

## Quick start

```bash
cargo run                                     # start the server (same as `serve`)

# in another terminal
cargo run -- share add Photos ~/Pictures      # share a folder (read-only)
cargo run -- share add Inbox ~/Inbox --writable
cargo run -- pair                             # shows a 6-digit code; approve the device
cargo run -- devices                          # list paired devices
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

Client developers: the exact protocol is in [docs/auth-protocol.md](docs/auth-protocol.md).

## Configuration

`config.json` in the data folder (created on first run):

| Field | Default | |
|---|---|---|
| `bind` | `0.0.0.0:7878` | Device API address. Use a specific IP (e.g. your Tailscale `100.x.y.z`) to listen on one interface only. |
| `admin_bind` | `127.0.0.1:7879` | Must be loopback. |
| `session_lifetime_secs` | `3600` | |
| `auth_rate_limit_per_minute` | `20` | Pairing/login requests per IP. |
| `max_list_limit`, `max_dir_entries`, `request_timeout_secs`, `max_request_body_bytes` | | Resource limits |

Environment overrides: `RIFTVIEW_DATA_DIR`, `RIFTVIEW_BIND`, `RUST_LOG`.

Data folder (owner-only permissions):

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
| GET | `/api/files/{id}/download` (supports `Range`) | `read` |

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
  storage/     atomic JSON/byte files
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
