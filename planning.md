# Rust Backend Engineering Specification
## Self-Hosted Cross-Platform File Access Service

**Project:** Open-Source File Sharing Application  
**Language:** Rust  
**HTTP Framework:** Axum  
**Async Runtime:** Tokio  
**Serialization:** Serde  
**TLS:** Rustls  
**Target Platforms:** Windows, macOS, Linux  
**Deployment:** Native background service (planned, Phase 8; today it runs from a terminal)  
**Database:** Embedded SQLite (`riftview.db`, no database server)  
**External Infrastructure:** None required

---

> **Status (2026-11-19):** Phases 1–5 are built. Phases 6–9 are not started. Each section below says what is built, what is planned, and where the build deliberately differs from the original plan.

# 1. Backend Overview

## 1.1 Objective

Develop a lightweight, cross-platform Rust backend that provides secure remote access to a computer's filesystem through an HTTP API.

The backend runs as a native background process on the host computer. It exposes explicitly authorized directories and provides endpoints for browsing, retrieving, uploading, downloading, and managing files.

The service communicates over HTTPS and is designed to operate over local networks or user-managed VPN connections.

The backend must not depend on:
- Developer-operated cloud infrastructure.
- Redis.
- PostgreSQL, MySQL, or MongoDB.
- Docker or other containers.
- Centralized authentication servers.
- External file storage services.

The service should be independently installable and operational on Windows, macOS, and Linux.

## 1.2 Core Responsibilities

The Rust backend is responsible for:

1. Hosting the HTTP API.
2. Managing network connections.
3. Authenticating trusted devices.
4. Enforcing device-specific permissions.
5. Managing shared filesystem roots.
6. Browsing directories and retrieving metadata.
7. Performing filesystem operations.
8. Streaming file uploads and downloads.
9. Managing resumable transfers.
10. Persisting application state.
11. Enforcing security restrictions.
12. Managing its own lifecycle as a desktop background service.

## 1.3 Architectural Principles

- Use one shared Rust codebase across desktop operating systems.
- Separate API handling from filesystem operations.
- Stream large files rather than loading them into memory.
- Keep memory consumption predictable.
- Use asynchronous networking and I/O where appropriate.
- Avoid unnecessary background processes.
- Persist only the state required for operation and recovery.
- Enforce filesystem access restrictions at the backend level.
- Never trust client-supplied filesystem paths or permissions.

---

# 2. Technology Stack

| Technology | Purpose |
|---|---|
| Rust | Backend implementation |
| Axum | REST API and HTTP routing |
| Tokio | Asynchronous runtime |
| Serde | JSON serialization |
| Tokio FS | Asynchronous filesystem operations |
| Rust standard library | Path resolution and filesystem utilities |
| cap-std | Handle-based filesystem access that cannot escape a shared root |
| SQLite (rusqlite, bundled) | Embedded database for shares, devices and permissions |
| Rustls | TLS |
| Tracing | Structured logging |
| UUID | Unique resource identifiers |
| SHA-256 | File and chunk integrity verification |
| Tower | HTTP middleware and request processing |

## 2.1 Why Axum?

Axum provides a relatively lightweight, modular HTTP framework built around Tokio and Tower.

It supports:
- Asynchronous request handlers.
- Middleware.
- Request extraction.
- JSON responses.
- Streaming response bodies.
- Shared application state.
- Integration with the Tokio ecosystem.

The backend should use Axum primarily as its HTTP interface rather than putting application logic directly inside route handlers.

---

# 3. Backend Architecture

## 3.1 Component Architecture

Two listeners run side by side. Devices only ever reach the HTTPS one; the admin API is for the host user on the same computer.

```text
   PAIRED DEVICE (phone app)                HOST USER (same computer)
            |                                        |
   HTTPS, pinned self-signed cert           plain HTTP, loopback only
   0.0.0.0:7878                             127.0.0.1:7879 + admin token
            |                                        |
            v                                        v
   +--------------------+                  +--------------------+
   |  Device API (api/) |                  |  Admin API (admin/)|
   +--------------------+                  |  + CLI (cli.rs)    |
   | Middleware:        |                  +--------------------+
   | - request timeout  |                            |
   | - body size limit  |                            |
   | - per-IP rate limit|                            |
   |   (pairing/login)  |                            |
   +--------------------+                            |
            |                                        |
            v                                        |
   +--------------------+                            |
   | Login + permission |                            |
   | check per request  |                            |
   | (auth/extractor)   |                            |
   +--------------------+                            |
            |                                        |
     +------+---------+--------------+               |
     |                |              |               |
     v                v              v               v
 +-----------+  +-----------+  +--------------------------+
 | filesystem|  | transfers |  | auth: pairing, sessions, |
 | (cap-std) |  | stream,   |  | devices & permissions    |
 |           |  | ETag, hash|  | filesystem/roots: shares |
 +-----------+  +-----------+  +--------------------------+
     |                |                      |
     v                v                      v
 +--------------------------+       +-------------------+
 | Operating system         |       | SQLite            |
 | filesystem (shares only) |       | riftview.db       |
 +--------------------------+       +-------------------+
```

## 3.2 Module Responsibilities

### API Module (`api/`), built

- HTTP routing for devices.
- Request validation.
- Response serialization.
- HTTP status codes.
- Error translation (`error.rs`): JSON errors that never contain host paths.
- Request cancellation: abandoned folder listings, downloads and hash requests stop their work.

### Admin Module (`admin/`, `cli.rs`), built (not in the original plan)

- Host-only API on loopback, protected by an admin token file.
- Shares: create, rename, toggle writable, remove.
- Devices: list, set permissions, set alias, revoke.
- Pairing: start, show status, approve, reject.
- The `riftview-api` CLI commands call this API.

### Authentication and Authorization Module (`auth/`), built

The original plan had separate authentication and authorization modules; they are one module.

- Device pairing (SPAKE2 six-digit code + host approval).
- Public-key identity management (Ed25519).
- Credential verification (signed login challenges).
- Session management (1-hour tokens).
- Device revocation.
- Device permissions, checked on every request by the `AuthenticatedDevice` extractor.
- Per-IP rate limiting for pairing and login.

Planned: per-share permissions for each device.

### Filesystem Module (`filesystem/`), partly built

Built:
- Directory browsing (paged, sorted, stoppable).
- File metadata.
- Safe path resolution (`cap-std`).
- File opening.
- Shares (`roots.rs`, stored in SQLite).
- Opaque file IDs.

Planned (Phase 7): file creation, rename and move, deletion, directory creation.

### Transfer Module (`transfers/`), downloads built

Built:
- File streaming in bounded chunks.
- File versions (`ETag`) and resume safety.
- Integrity verification (SHA-256 + in-memory cache).
- Concurrent download limits.
- Transfer cancellation (download stops when the device disconnects).

Planned (Phase 6): transfer initialization, chunk management, upload progress, resumable uploads, temporary file management.

### Database Module (`database/`), built (not in the original plan)

- SQLite connection with WAL mode and foreign keys.
- Numbered schema migrations tracked with `PRAGMA user_version`.

### Storage Module (`storage/`), built

- Atomic writes for `config.json`, `admin_token` and the TLS certificate.

### Platform Module (`platform/`), partly built

Built: application data directories and owner-only file permissions per OS.

Planned (Phase 8): service installation, startup integration, other platform-specific filesystem permissions.

---

# 4. Project Structure

Current layout. Entries marked *(planned)* don't exist yet.

```text
riftview-api/
|
├── Cargo.toml
├── Cargo.lock
|
├── src/
|   ├── main.rs            command-line entry point
|   ├── lib.rs
|   ├── cli.rs             pair, share, devices, rename, revoke, info commands
|   ├── server.rs          listeners and middleware
|   ├── config.rs          config.json
|   ├── state.rs           shared application state
|   ├── error.rs           errors as clients see them
|   ├── tls.rs             self-signed certificate and pin
|   |
|   ├── api/               device API (HTTPS), thin handlers
|   |   ├── mod.rs
|   |   ├── health.rs
|   |   ├── pairing.rs
|   |   ├── auth.rs
|   |   ├── shares.rs
|   |   └── files.rs       browse, metadata, download, hash
|   |
|   ├── admin/             host-only admin API (loopback + admin token)
|   |   ├── mod.rs
|   |   ├── token.rs
|   |   ├── shares.rs
|   |   ├── devices.rs
|   |   └── pairing.rs
|   |
|   ├── auth/              authentication and authorization
|   |   ├── mod.rs
|   |   ├── protocol.rs    SPAKE2 identities and transcript labels
|   |   ├── crypto.rs
|   |   ├── pairing.rs
|   |   ├── sessions.rs
|   |   ├── devices.rs     devices and permissions (SQLite)
|   |   ├── extractor.rs   per-request login and permission checks
|   |   └── rate_limit.rs
|   |
|   ├── filesystem/        no HTTP knowledge
|   |   ├── mod.rs
|   |   ├── browser.rs
|   |   ├── resolver.rs
|   |   ├── metadata.rs
|   |   ├── ids.rs         opaque file IDs
|   |   ├── operations.rs
|   |   └── roots.rs       shares (SQLite)
|   |
|   ├── transfers/
|   |   ├── mod.rs
|   |   ├── stream.rs      streaming download body
|   |   ├── version.rs     file versions / ETags
|   |   ├── hashing.rs     SHA-256 + cache
|   |   ├── limits.rs      concurrent download limits
|   |   └── upload.rs, chunks.rs, persistence.rs   (planned, Phase 6)
|   |
|   ├── database/
|   |   ├── mod.rs         connection, pragmas, migrations
|   |   └── migrations/
|   |       └── 001_devices_and_shares.sql
|   |
|   ├── storage/           atomic writes for config, admin token, certificate
|   |   └── mod.rs
|   |
|   └── platform/
|       ├── mod.rs
|       ├── windows.rs
|       ├── macos.rs
|       └── linux.rs
|
├── tests/
|   ├── common/mod.rs      reference client used by the tests
|   ├── api_tests.rs
|   ├── auth_tests.rs
|   └── security_tests.rs
|
└── docs/
    └── auth-protocol.md
```

Authorization lives in `auth/` (`devices.rs` holds permissions, `extractor.rs` enforces them) rather than a separate `authorization/` module.

The exact module boundaries can evolve during development. Avoid introducing abstractions before they provide a practical benefit.

---

# 5. HTTP API Design

The backend exposes two APIs:

- the **device API**, over HTTPS, used by paired devices;
- the **admin API**, over loopback HTTP with an admin token, used by the host user's CLI.

Every device API endpoint except health, version, pairing and login requires a logged-in device, and checks that device's permissions. Errors are JSON: `{"error":{"code":"…","message":"…"}}`.

**Change from the original plan:** device management and share management moved from the device API to the admin API. A phone should not be able to list or revoke other devices, or decide which folders are shared.

## 5.1 Health and Host Information

| Method | Endpoint | Access | Status |
|---|---|---|---|
| GET | `/api/health` | public | Built |
| GET | `/api/version` | public | Built |
| GET | `/api/host` | logged in | Built |

The health endpoint returns only `{"status":"ok"}` and exposes no host details.

## 5.2 Pairing, Login and Devices

| Method | Endpoint | Access | Status |
|---|---|---|---|
| POST | `/api/pairing/initiate` | public, rate-limited | Built |
| POST | `/api/pairing/complete` | public, rate-limited | Built |
| GET | `/api/pairing/{id}` | public, rate-limited | Built: poll until the host approves |
| POST | `/api/auth/challenge` | public, rate-limited | Built |
| POST | `/api/auth/session` | public, rate-limited | Built: log in |
| DELETE | `/api/auth/session` | logged in | Built: log out |
| GET | `/api/devices/me` | logged in | Built |

Not built (by design): `GET /api/devices`, `GET /api/devices/{id}`, `DELETE /api/devices/{id}` on the device API. These are in the admin API (§5.6).

## 5.3 Shared Directories

| Method | Endpoint | Permission | Status |
|---|---|---|---|
| GET | `/api/shares` | `browse` | Built |
| GET | `/api/shares/{id}` | `browse` | Built |

Not built (by design): `POST`, `PATCH` and `DELETE` on `/api/shares`. Only the host user manages shares, through the admin API (§5.6).

## 5.4 Filesystem Operations

| Method | Endpoint | Permission | Status |
|---|---|---|---|
| GET | `/api/shares/{id}/files?path=&sort=&order=&offset=&limit=` | `browse` | Built |
| GET | `/api/files/{id}` | `browse` | Built |
| GET | `/api/files/{id}/download` | `read` | Built: `Range`, `If-Range`, `ETag` |
| GET | `/api/files/{id}/hash` | `read` | Built: `{sha256, size, etag}` (not in the original plan) |
| POST | `/api/files/{id}/rename` | `rename` | Planned, Phase 7 |
| POST | `/api/files/{id}/move` | `move` | Planned, Phase 7 |
| POST | `/api/shares/{id}/directories` | `upload` | Planned, Phase 7 |
| DELETE | `/api/files/{id}` | `delete` | Planned, Phase 7 |

File identifiers are an opaque base64url encoding of (share ID, relative path). They are decoded and re-checked on every request, and clients never send host paths.

## 5.5 Transfers (uploads)

Planned, Phase 6.

| Method | Endpoint | Purpose |
|---|---|---|
| POST | `/api/transfers` | Initialize transfer |
| GET | `/api/transfers/{id}` | Retrieve transfer status |
| PUT | `/api/transfers/{id}/chunks/{index}` | Upload chunk |
| GET | `/api/transfers/{id}/chunks/{index}` | Retrieve chunk |
| POST | `/api/transfers/{id}/complete` | Finalize transfer |
| DELETE | `/api/transfers/{id}` | Cancel transfer |

## 5.6 Admin API (host only)

Built. Listens on `127.0.0.1:7879` (must be loopback) and needs `Authorization: Bearer <admin_token>`.

| Method | Endpoint | Purpose | CLI |
|---|---|---|---|
| GET | `/admin/host` | Host info and certificate pin | `info` |
| GET, POST | `/admin/shares` | List / add shares | `share list`, `share add` |
| GET, PATCH, DELETE | `/admin/shares/{id}` | Show / rename or toggle writable / remove | `share remove` (no CLI for PATCH yet) |
| GET | `/admin/devices` | List devices | `devices` |
| PATCH | `/admin/devices/{id}` | Set permissions or alias | `rename` |
| DELETE | `/admin/devices/{id}` | Revoke | `revoke` |
| GET, POST, DELETE | `/admin/pairing` | Pairing status / start / cancel | `pair` |
| POST | `/admin/pairing/approve` | Approve with permissions | `pair` |
| POST | `/admin/pairing/reject` | Reject | `pair` |

---

# 6. Filesystem Implementation

## 6.1 Shared-Root Model

The backend must only expose directories explicitly configured by the host user.

Example:

```text
Shared Root: root_123

/Users/user/Shared/
|
├── Documents/
├── Pictures/
├── Videos/
└── report.pdf
```

The client should identify the shared root using an opaque identifier and request paths relative to that root.

Example request:

```http
GET /api/shares/root_123/files?path=/Documents
```

The backend resolves the requested directory and returns its contents.

## 6.2 Directory Browsing

The backend should use Rust filesystem APIs to retrieve directory entries.

Relevant APIs:

- `tokio::fs::read_dir`
- `tokio::fs::metadata`
- `std::fs`
- `std::path::Path`
- `std::path::PathBuf`

Example response:

```json
{
  "share_id": "root_123",
  "path": "/Documents",
  "entries": [
    {
      "name": "report.pdf",
      "type": "file",
      "size": 2048000
    },
    {
      "name": "Projects",
      "type": "directory"
    }
  ]
}
```

Directory browsing should support:
- Pagination or bounded result sizes.
- Sorting.
- Metadata retrieval.
- Consistent error handling.
- Cancellation when requests are abandoned.

Do not recursively scan entire directory trees during ordinary browsing.

**As built:**
- Listings go through `cap-std` on a background thread. The response also carries `total`, `offset`, `limit` and an opaque `id` per entry.
- Sort by `name`, `size` or `modified`, ascending or descending. Folders always come first.
- Page size defaults to 200, with a configurable maximum (`max_list_limit`, 1000).
- Folders with more than `max_dir_entries` (100,000) entries are refused with `409`.
- Entries that can't be read, names that aren't UTF-8, and symlinks pointing outside the share are skipped.
- If the request is abandoned (the device disconnects or the request times out), the scan stops within one entry.
- Not built: a short-lived listing cache. Each page request re-reads and re-sorts the whole folder.

## 6.3 Path Resolution

Path resolution is a security-critical component.

The backend must prevent:
- `../` traversal.
- Absolute path injection.
- Symlink escapes.
- Access to unauthorized directories.
- Race conditions between checking a path and opening it.

A simple string-prefix comparison is not sufficient.

Use directory-relative, handle-based filesystem operations where supported. On Linux, consider `openat2` with restrictive resolution flags. On Windows and macOS, use appropriate platform-specific handle-based approaches when needed.

Where equivalent guarantees are not available through a portable abstraction, isolate the platform-specific implementation.

The filesystem module should expose safe operations rather than allowing API handlers to construct arbitrary filesystem paths.

**As built:** every access goes through a `cap_std::fs::Dir` handle on the share root, so the OS refuses lookups that leave it (`..`, absolute paths, escaping symlinks). Relative paths are parsed strictly: `..`, backslashes, NUL bytes and drive prefixes are rejected, not cleaned up. Files are type-checked after opening, so there's no gap between checking and opening. Named pipes are opened non-blocking and rejected. Shares can't overlap each other or contain the data folder.

## 6.4 File Metadata

Retrieve metadata without reading file contents.

Recommended fields:

- Name.
- Size.
- Last modified time.
- File or directory classification.
- MIME type, when available.
- File identifier.

Avoid calculating hashes for every file during directory browsing. Hash files on demand or as part of transfer verification.

---

# 7. Authentication and Device Trust

The exact wire protocol is in `docs/auth-protocol.md`; client requirements are in `mobile_app_reqs.md`.

## 7.1 Pairing Workflow

The initial pairing process should be explicit.

1. Generate a temporary six-digit code on the desktop.
2. Display the code locally.
3. Accept a pairing request from a client.
4. Validate the code.
5. Require explicit desktop approval.
6. Establish a cryptographic device identity.
7. Persist the trusted device's public key.
8. Assign initial permissions.

Pairing codes must be:
- Short-lived.
- Single-use.
- Rate-limited.
- Invalidated after successful pairing.
- Excluded from application logs.

A six-digit code provides limited entropy, so it must never serve as a long-term authentication credential.

**As built:**
1. `riftview-api pair` creates a six-digit code, valid for 5 minutes, and shows it only in the host's terminal. It is never logged.
2. The device and host run **SPAKE2** (Ed25519 group, `spake2` crate) with the code as the password. The code itself never crosses the network, so someone eavesdropping can't recover it offline.
3. The device proves it knew the code with an HMAC over its Ed25519 public key, its name and the certificate pin it saw. This binds the pairing to that exact certificate, which defeats a man-in-the-middle.
4. **One guess per code:** a wrong proof burns the code. Pairing and login routes share a per-IP rate limit (20 per minute).
5. The host user approves (read-only or full access) or rejects in the terminal. The device polls `GET /api/pairing/{id}`.
6. On approval, the host returns the new device ID and its own HMAC proof, and the device stores the certificate pin.

## 7.2 Cryptographic Identity

Each trusted device should possess a public/private key pair.

The private key stays on the device and is stored in secure platform storage.

The desktop stores:
- Device ID.
- Public key.
- Display name.
- Permissions.
- Pairing timestamp.
- Revocation status.

Use an established authentication protocol or a well-reviewed cryptographic library. Do not invent a custom challenge-response protocol.

**As built:** Ed25519 keys (`ed25519-dalek`). In addition to the fields above, the `devices` table in SQLite stores:
- a host-chosen alias
- the key version (always 1 for now)
- device type, platform and app version
- the last connection time

Revoked devices keep their row, with `revoked_at_ms` set. See `data_planning.md`.

## 7.3 Session Management

The backend may issue short-lived sessions after verifying device identity.

Sessions should:
- Expire.
- Be revocable.
- Be bound to the authenticated device.
- Carry no more authority than the device's permissions.
- Be protected against replay.

**As built:**
- The device asks for a random challenge (single-use, valid for 60 seconds) and signs it with its Ed25519 key.
- The signature also covers the device ID and the pinned certificate hash.
- In return the device gets a bearer token that lasts 1 hour (`session_lifetime_secs`). Tokens are kept in memory only, so a restart logs devices out.
- Revoking a device ends its sessions immediately.
- Permissions are re-read on every request, so changes apply right away.

## 7.4 Authorization

Authentication and authorization are separate responsibilities.

A successfully authenticated device must not automatically have unrestricted access.

| Permission | Description | Status |
|---|---|---|
| Browse | List shares and folders, see file details | Built, enforced |
| Read | Download files and get their SHA-256 | Built, enforced |
| Upload | Create new files and folders | Stored and checked; endpoints planned (Phases 6–7) |
| Rename | Rename files | Stored and checked; endpoints planned (Phase 7) |
| Move | Move files | Stored and checked; endpoints planned (Phase 7) |
| Delete | Delete files | Stored and checked; endpoints planned (Phase 7) |
| Manage Shares | Modify shared roots | **Not a device permission.** Only the host user can manage shares (admin API). |

Permissions should be enforced by the backend for every operation.

**As built:**
- Permissions are stored one row per permission (`device_permissions`), and apply to all shares.
- Each share also has a `writable` flag (off by default). Upload, rename, move and delete only work when the device has the permission **and** the share is writable.
- Pairing approval offers two presets: read-only (`browse`, `read`) and full access (all six).

---

# 8. File Transfer Engine

## 8.1 Design Requirements

The transfer engine must support:
- Large files.
- Bounded memory usage.
- Streaming.
- Chunked uploads.
- Resumable transfers.
- Integrity verification.
- Cancellation.
- Concurrent transfers.
- Transfer persistence.

The transfer engine should be independent of the HTTP route handlers.

## 8.2 Download Implementation

A download should follow this sequence:

1. Authenticate the client.
2. Verify read permission.
3. Resolve the file safely.
4. Open the file.
5. Stream file contents to the HTTP response.
6. Support range requests where appropriate.
7. Cancel underlying I/O when the client disconnects.

Do not load the entire file into a `Vec<u8>`.

Use bounded buffers and streaming response bodies.

**As built** (`api/files.rs`, `transfers/stream.rs`):
- The file is opened through `cap-std` and streamed in 64 KiB chunks from one open handle, so memory stays flat regardless of file size.
- One `Range` per request (`bytes=A-B`, `A-`, `-N`). Several ranges, or a malformed header, get the whole file. A start past the end gets `416` with the real size.
- Every response carries an `ETag` (a hash of size, modified time, inode and change time). `If-Range` with a stale ETag gets the whole file (`200`) instead of a mismatched part.
- If the file is edited in place while streaming, the connection is cut before the last bytes, so the client never sees a complete-looking corrupted file. If the file is replaced (rename-over), the original is still served consistently.
- When the client disconnects, the stream is dropped and reading stops.

## 8.3 Upload Implementation

An upload should follow this sequence:

1. Authenticate the client.
2. Verify upload permission.
3. Initialize a transfer.
4. Create a temporary file in an authorized destination.
5. Receive chunks.
6. Validate chunk indexes and lengths.
7. Persist completed chunk state.
8. Verify the completed file.
9. Finalize the file atomically.
10. Remove temporary state.

Do not expose partially uploaded files as completed files.

## 8.4 Chunking

Initial recommended chunk size: 4 MiB.

For a 50 GiB file:

```text
File size:       50 GiB
Chunk size:       4 MiB
Total chunks: 12,800
```

The backend should maintain a transfer manifest containing:

- Transfer ID.
- Source or destination information.
- Expected file size.
- Chunk size.
- Completed chunk indexes.
- Per-chunk integrity information.
- Final file hash.
- Transfer status.
- Expiration information.

The client should be able to request the status of a transfer and retransmit missing chunks.

## 8.5 Integrity Verification

Use SHA-256 or another established cryptographic hash.

Verify:
- Individual chunks where applicable.
- The final assembled file.
- Expected file size.

For downloads, account for the possibility that the source file changes during a transfer. Use a stable file handle and a clearly defined source-consistency strategy.

**As built (downloads):** `GET /api/files/{id}/hash` returns the SHA-256 of the whole file, plus its size and ETag. Hashing streams the file with a 1 MiB buffer and stops if the request is abandoned. It is exempt from the request timeout. Results are cached in memory (up to 10,000, least recently used evicted) until the file's version changes. If the file changes during hashing, the request returns `409`. The download stream and ETag above are the source-consistency strategy. Per-chunk upload verification is planned with Phase 6.

For uploads, finalize using an atomic rename where supported, while accounting for platform-specific behavior and destination conflicts.

## 8.6 Resource Limits

Implement:
- Maximum concurrent transfers.
- Maximum upload size.
- Maximum chunk size.
- Maximum active transfer count.
- Transfer expiration.
- Per-device limits.
- Request timeouts.
- Bounded buffering.

Memory consumption should not increase proportionally to the total size of a file being transferred.

**As built:**

| Limit | Status |
|---|---|
| Concurrent downloads, total | Built: `max_concurrent_downloads`, 16 (includes hash requests) |
| Concurrent downloads, per device | Built: `max_downloads_per_device`, 4 (includes hash requests) |
| Request timeouts | Built: 30 s until the response starts (`request_timeout_secs`); `/hash` exempt; streaming isn't cut off |
| Bounded buffering | Built: 64 KiB download chunks, 1 MiB hash buffer |
| Request body size | Built: 64 KiB (`max_request_body_bytes`) |
| Upload size, chunk size, active transfers, expiration | Planned, Phase 6 |

Over-limit requests get `429 too_many_requests`.

---

# 9. Persistent State

No database server is required. Relational data lives in an embedded SQLite database (`riftview.db`), and a few small settings files sit beside it. Column-by-column details are in [data_planning.md](data_planning.md).

Data folder (`~/Library/Application Support/RiftView/` on macOS, or `RIFTVIEW_DATA_DIR`):

```text
RiftView/
|
├── riftview.db           SQLite: shares, devices, device_permissions
├── riftview.db-wal       SQLite write-ahead log (while the server runs)
├── riftview.db-shm
├── config.json           server settings
├── admin_token           secret for the host-only admin API
|
└── tls/
    ├── cert.pem          self-signed certificate
    └── key.pem           its private key
```

Database tables (schema in `src/database/migrations/`):

| Table | Holds |
|---|---|
| `shares` | Shared folders: ID, name, host path, writable |
| `devices` | Trusted devices: ID, names, Ed25519 public key, type, platform, app version, timestamps, revocation |
| `device_permissions` | One row per permission a device has |

Persist:
- Host configuration (`config.json`).
- Shared directory definitions (SQLite).
- Trusted device public keys (SQLite).
- Device permissions (SQLite).
- Transfer metadata and resumable transfer state *(planned, Phase 6; new relational state goes in SQLite)*.

Not persisted: login sessions, pairing sessions and the SHA-256 cache. They live in memory, so a restart logs devices out and they sign in again.

Use:
- Numbered schema migrations, tracked with `PRAGMA user_version`. Never edit a shipped migration; add a new one.
- SQLite transactions and WAL mode for safe recovery after unexpected termination.
- Atomic writes for the settings files.
- Versioned configuration schemas.
- Owner-only file permissions on the whole data folder.

Never persist private device keys or plaintext passwords. The host only stores device *public* keys; private keys stay on each device.

The backend should validate persisted transfer metadata against actual files before resuming operations.

---

# 10. Caching and Resource Management

Redis should not be part of the initial architecture.

Use in-process caching only when performance measurements justify it.

Potential cache targets:
- Directory listings.
- Frequently requested metadata.
- Shared-root configuration.
- Permission lookups.

Directory caches should have short expiration periods or invalidation mechanisms because files can be changed outside the application.

**As built:**
- In memory: SHA-256 results (invalidated by file version), shares and devices (loaded from SQLite at startup, written through on change), sessions and pairing state.
- Not built: a directory listing cache.

Resource-management priorities:

1. Stream file contents.
2. Use bounded buffers.
3. Limit concurrent transfers.
4. Avoid unnecessary filesystem scans.
5. Bound directory listings.
6. Limit cache memory.
7. Cancel abandoned operations.
8. Profile actual memory usage.

Performance targets should be established through measurements on real operating systems and hardware.

---

# 11. TLS and Network Security

The backend must use HTTPS for all authenticated API communication.

Important requirements:

- Use Rustls or another maintained TLS implementation.
- Never disable certificate verification.
- Establish a secure host identity and certificate-validation strategy for LAN and VPN connections.
- Bind only to interfaces explicitly configured by the user.
- Do not expose an unauthenticated listener to public networks.
- Apply rate limits to pairing and authentication.
- Apply request-size limits.
- Set appropriate connection and request timeouts.
- Avoid exposing internal filesystem paths in errors.
- Ensure secrets are not included in logs.

A self-hosted application still needs a deliberate certificate-trust model. Self-signed certificates alone do not establish trust unless the client securely verifies the expected host identity.

**As built:**
- **Trust model:** on first run the host creates a self-signed ECDSA P-256 certificate (names `riftview.local`, `localhost`), stored in `tls/`. The device sees its public key hash (SPKI SHA-256) and binds it into the SPAKE2 pairing proof (§7.1). From then on, the device connects only if the presented key matches that pin. The host user can see the pin with `riftview-api info`.
- **Rustls** with the `ring` provider.
- **Listeners:**
  - The device API binds `0.0.0.0:7878` by default. **This differs from "bind only to interfaces explicitly configured by the user":** it was changed so phones on the LAN or a VPN can connect without editing settings. Set `bind` (or `RIFTVIEW_BIND`) to one address, such as a Tailscale `100.x.y.z`, to restrict it.
  - The admin API must be loopback; startup fails otherwise.
- **Unauthenticated routes:** only health, version, pairing and login. The last two are rate-limited per IP.
- **Limits:** request body 64 KiB; 30-second response-start timeout.
- **Errors and logs:** errors never contain host paths or raw OS errors. Pairing codes, tokens and keys are never logged.

---

# 12. Cross-Platform Implementation

The backend should use one shared codebase.

Most filesystem operations can use Rust's standard library and Tokio filesystem APIs.

Platform-specific implementations should be limited to functionality that genuinely differs.

| Feature | Windows | macOS | Linux |
|---|---|---|---|
| Directory browsing | Shared | Shared | Shared |
| File metadata | Shared | Shared | Shared |
| File streaming | Shared | Shared | Shared |
| Transfer management | Shared | Shared | Shared |
| Path resolution | Platform-aware | Platform-aware | Platform-aware |
| Startup integration | Windows-specific | LaunchAgent | systemd |
| Permissions | Windows ACLs | macOS permissions | POSIX permissions |
| Native packaging | Windows installer | macOS package | Linux package |

## 12.1 Build Targets

```bash
# macOS Apple Silicon
cargo build --release --target aarch64-apple-darwin

# macOS Intel
cargo build --release --target x86_64-apple-darwin

# Windows x64
cargo build --release --target x86_64-pc-windows-msvc

# Linux x64
cargo build --release --target x86_64-unknown-linux-gnu
```

Cross-compilation requires the appropriate toolchains and target dependencies.

All supported platforms should receive native integration testing.

---

# 13. Testing Strategy

## 13.1 Unit Tests

Test:
- Path resolution.
- Shared-root confinement.
- Permission enforcement.
- Device authentication.
- Pairing-code expiration.
- Transfer state.
- Chunk validation.
- Configuration loading.

## 13.2 Integration Tests

Test:
- API endpoints.
- Directory browsing.
- File metadata.
- Uploads.
- Downloads.
- Interrupted transfers.
- Device revocation.
- Concurrent requests.
- File conflicts.

## 13.3 Security Tests

Test:
- Directory traversal attempts.
- Symlink escapes.
- Unauthorized absolute paths.
- Replay attempts.
- Expired credentials.
- Revoked devices.
- Oversized uploads.
- Malformed requests.
- Race conditions involving filesystem changes.

## 13.4 Performance Tests

Measure:
- Idle RSS memory.
- Memory under concurrent transfers.
- CPU utilization.
- Directory listing latency.
- Upload throughput.
- Download throughput.
- Recovery after interrupted transfers.

Benchmark different chunk sizes and concurrency limits.

**As built:** 81 tests (`cargo test`).
- **Unit tests:** path parsing, Range parsing, ETags, hash cache, download limits, stop signal, migrations, crypto, dates.
- **`tests/api_tests.rs`:** browsing, metadata, downloads, resume, hashing, a file changing mid-download, limits, a 5 GiB sparse file.
- **`tests/auth_tests.rs`:** pairing and attack cases, login, replay, expiry, revocation, persistence across restart.
- **`tests/security_tests.rs`:** traversal, symlink escapes, named pipes, forged IDs.

Not yet covered:
- config loading tests
- a race test for swapping a folder for a symlink mid-lookup
- uploads, conflicts
- performance measurements (§13.4)

---

# 14. Development Roadmap

## Phase 1: Rust Project Foundation

Tasks:
- Initialize Cargo project.
- Configure Axum and Tokio.
- Establish application state.
- Create server startup.
- Add structured logging.
- Add configuration loading.
- Implement health endpoint.

Deliverable: A working Rust HTTP server.

**Status: done.**

## Phase 2: Filesystem Abstraction

Tasks:
- Implement shared-root configuration.
- Implement directory browsing.
- Retrieve file metadata.
- Implement safe path resolution.
- Implement file opening.
- Add filesystem error handling.
- Implement path-confinement tests.

Deliverable: A secure filesystem module independent of HTTP.

**Status: done.** Remaining: a race test for path lookups.

## Phase 3: REST API

Tasks:
- Implement filesystem routes.
- Add request validation.
- Add JSON serialization.
- Add structured errors.
- Implement pagination or bounded directory listings.
- Add API integration tests.

Deliverable: A functional filesystem REST API.

**Status: done**, including abandoned-listing cancellation. Remaining: `ETag` on `GET /api/files/{id}`, config loading tests.

## Phase 4: Authentication and Authorization

Tasks:
- Implement temporary pairing codes.
- Implement pairing expiration.
- Implement device identities.
- Implement cryptographic verification.
- Persist trusted devices.
- Implement permission policies.
- Implement revocation.

Deliverable: A device-authenticated API with authorization enforcement.

**Status: done.** Devices are persisted in SQLite. Share and device management are host-only (admin API + CLI).

## Phase 5: Download Engine

Tasks:
- Implement streaming downloads.
- Implement bounded buffering.
- Implement HTTP range support.
- Implement transfer cancellation.
- Add integrity verification.
- Test large files.

Deliverable: Reliable large-file downloads.

**Status: done.** Streaming, bounded buffering, Range, `ETag`/`If-Range`, stop on disconnect, SHA-256 hash endpoint, concurrent download limits, 5 GiB test. Remaining: real-world memory and throughput measurements (Phase 9).

## Phase 6: Upload Engine

Tasks:
- Implement transfer initialization.
- Implement chunk uploads.
- Implement chunk verification.
- Implement transfer persistence.
- Implement resumability.
- Implement atomic finalization.
- Handle destination conflicts.

Deliverable: Reliable resumable uploads.

**Status: not started.** Transfer state will go in SQLite.

## Phase 7: Filesystem Management

Tasks:
- Implement rename.
- Implement move.
- Implement directory creation.
- Implement deletion.
- Enforce operation-specific permissions.
- Handle conflicts and partial failures.

Deliverable: Complete authorized filesystem management.

**Status: not started.** The permissions and the per-share `writable` flag already exist and are checked.

## Phase 8: Cross-Platform Integration

Tasks:
- Build Windows target.
- Build macOS targets.
- Build Linux target.
- Implement native service integration.
- Handle OS-specific permissions.
- Test path behavior.
- Test filesystem security.
- Package native executables.

Deliverable: Cross-platform native host service.

**Status: not started.** Built and tested on macOS only. Per-OS data-folder code exists.

## Phase 9: Optimization and Release

Tasks:
- Profile memory consumption.
- Benchmark transfer performance.
- Test concurrent connections.
- Audit dependencies.
- Review security boundaries.
- Improve error handling.
- Document configuration.
- Prepare installation instructions.
- Publish source code and release builds.

Deliverable: Stable open-source Rust backend.

**Status: not started.**

---

# 15. Initial MVP Requirements

The initial backend release should support:

- Native Rust host executable.
- HTTPS API.
- Shared-directory configuration.
- Directory browsing.
- File metadata.
- Device pairing.
- Device authentication.
- Device revocation.
- Read and write authorization.
- File downloads.
- File uploads.
- Transfer progress.
- Basic resumability.
- Cross-platform compilation.

Defer:
- Automatic host discovery.
- File synchronization.
- Distributed host clustering.
- Cloud relay infrastructure.
- Advanced indexing.
- Automatic compression.
- MCP integration.
- Complex file previews.

---

# 16. Final Architecture Recommendation

| Component | Selected Technology |
|---|---|
| Language | Rust |
| HTTP server | Axum |
| Async runtime | Tokio |
| JSON | Serde |
| HTTP middleware | Tower |
| TLS | Rustls (ring), self-signed ECDSA P-256 certificate, pinned by clients |
| Pairing | SPAKE2 (six-digit code) + host approval |
| Device identity | Ed25519 |
| Logging | Tracing |
| Identifiers | UUID |
| Integrity | SHA-256 |
| Filesystem | cap-std (confined), std::fs / tokio::fs |
| Persistence | Embedded SQLite (`riftview.db`) + small settings files |
| Cache | In-process, if needed |
| Database | SQLite via rusqlite (embedded, no server) |
| Host management | Loopback admin API + `riftview-api` CLI |
| Deployment | Native background process (planned; terminal today) |

## Engineering Priorities

The development process should prioritize these concerns in order:

1. Filesystem security and path confinement.
2. Device authentication and authorization.
3. Correct filesystem operations.
4. Reliable streaming and resumable transfers.
5. Cross-platform compatibility.
6. Predictable memory consumption.
7. Operational simplicity.

The API should remain independent of the filesystem implementation, and the filesystem implementation should remain independent of the HTTP framework.

This separation is the foundation for a modular, secure, lightweight, and extensible Rust backend.

**The ultimate objective is to provide a secure device-to-device filesystem service that runs natively on user-owned computers without requiring centralized infrastructure or unnecessary background services.**