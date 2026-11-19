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

> **Status (2026-11-19):** Phases 1–7 are built. Phases 8–9 are not started. Each section below says what is built, what is planned, and where the build deliberately differs from the original plan.

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

### Filesystem Module (`filesystem/`), built

- Directory browsing (paged, sorted, stoppable).
- File metadata.
- Safe path resolution (`cap-std`).
- File opening.
- Shares (`roots.rs`, stored in SQLite).
- Opaque file IDs.
- File creation for uploads (`writing.rs`).
- Rename, move, directory creation and deletion (`managing.rs`). Deletion goes to the Trash of the item's disk (`trash.rs`).
- Copying between disks for moves (`copying.rs`).
- Small OS-specific helpers (`os.rs`): a rename that never replaces, free space, and a folder's real path.

### Jobs Module (`jobs/`), built (not in the original plan)

- Background moves between shares on different disks: copy to a hidden temporary name, give it the real name, delete the original.
- Records in SQLite (`move_jobs`), live progress in memory, cancellation.
- Cleanup of jobs interrupted by a restart.

### Transfer Module (`transfers/`), built

Built:
- File streaming in bounded chunks.
- File versions (`ETag`) and resume safety.
- Integrity verification (SHA-256 + in-memory cache).
- Concurrent transfer limits (downloads, hashes and upload chunks share them).
- Transfer cancellation (download stops when the device disconnects).
- Uploads: initialization, chunk tracking in SQLite, progress, resumability, cleanup of cancelled/expired/failed uploads (`uploads.rs`, `cleanup.rs`).

Writing the partial and final files lives in `filesystem/writing.rs`, so it stays confined by `cap-std` like all other file access.

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
|   |   ├── writing.rs     upload partial files, finishing
|   |   ├── managing.rs    rename, move, new folder, delete
|   |   ├── trash.rs       move to the disk's Trash (NSFileManager on macOS)
|   |   ├── copying.rs     copying between disks, tree removal
|   |   ├── os.rs          no-replace rename, free space, real path
|   |   └── roots.rs       shares (SQLite)
|   |
|   ├── transfers/
|   |   ├── mod.rs
|   |   ├── stream.rs      streaming download body
|   |   ├── version.rs     file versions / ETags
|   |   ├── hashing.rs     SHA-256 + cache
|   |   ├── limits.rs      concurrent transfer limits
|   |   ├── uploads.rs     unfinished uploads and chunks (SQLite)
|   |   └── cleanup.rs     cancelled / expired / failed uploads; housekeeping task
|   |
|   ├── jobs/
|   |   ├── mod.rs         background job records (SQLite) + live progress
|   |   └── moves.rs       move-between-disks runner, restart recovery
|   |
|   ├── database/
|   |   ├── mod.rs         connection, pragmas, migrations
|   |   └── migrations/
|   |       ├── 001_devices_and_shares.sql
|   |       ├── 002_uploads.sql
|   |       └── 003_move_jobs.sql
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
|   ├── security_tests.rs
|   ├── upload_tests.rs
|   └── manage_tests.rs
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
| POST | `/api/files/{id}/rename` | `rename` | Built: `{name}` → item; `409` if taken |
| POST | `/api/files/{id}/move` | `move` | Built: `{share_id?, path}` → `200` item (same disk) or `202` job (other disk); `409` if taken |
| POST | `/api/shares/{id}/directories` | `upload` | Built: `{path, name}` → `201` folder |
| DELETE | `/api/files/{id}` | `delete` | Built: file or whole folder → `{deleted: "trash" \| "permanent"}` |
| GET | `/api/jobs/{id}` | `move` | Built (not in the original plan): move job progress and result |
| DELETE | `/api/jobs/{id}` | `move` | Built (not in the original plan): cancel a running move |

All of these need the share (both shares, for a move) to be writable.

File identifiers are an opaque base64url encoding of (share ID, relative path). They are decoded and re-checked on every request, and clients never send host paths.

## 5.5 Transfers (uploads)

All need the `upload` permission; starting, sending chunks and finishing also need a writable share. A device only sees its own uploads (others are `404`).

| Method | Endpoint | Purpose | Status |
|---|---|---|---|
| POST | `/api/transfers` | Initialize transfer: share, folder, name, size, whole-file SHA-256 | Built |
| GET | `/api/transfers` | This device's unfinished uploads | Built (not in the original plan) |
| GET | `/api/transfers/{id}` | Transfer status, including missing chunk indexes | Built |
| PUT | `/api/transfers/{id}/chunks/{index}` | Upload chunk, with `X-Chunk-SHA256` | Built |
| GET | `/api/transfers/{id}/chunks/{index}` | Retrieve chunk | **Not built** (chunks are hash-checked on arrival, so reading them back isn't needed) |
| POST | `/api/transfers/{id}/complete` | Finalize transfer | Built |
| DELETE | `/api/transfers/{id}` | Cancel transfer | Built |

## 5.6 Admin API (host only)

Built. Listens on `127.0.0.1:7879` (must be loopback) and needs `Authorization: Bearer <admin_token>`.

| Method | Endpoint | Purpose | CLI |
|---|---|---|---|
| GET | `/admin/host` | Host info and certificate pin | `info` |
| GET, POST | `/admin/shares` | List / add shares | `share list`, `share add` |
| GET, PATCH, DELETE | `/admin/shares/{id}` | Show / rename or toggle writable / remove | `share rename`, `share set-writable`, `share remove` |
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
| Upload | Create new files and folders | Built, enforced (needs a writable share) |
| Rename | Rename files and folders | Built, enforced (needs a writable share) |
| Move | Move files and folders, follow move jobs | Built, enforced (both shares writable) |
| Delete | Delete files and folders (to the Trash) | Built, enforced (needs a writable share) |
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

**As built** (`api/transfers.rs`, `transfers/uploads.rs`, `filesystem/writing.rs`):
1. **Start:**
   - Checks the `upload` permission, that the share is writable, and that the target folder exists (it isn't created).
   - Checks the name: no `/` or `..`, and not the reserved partial-file pattern.
   - Checks the size against `max_upload_bytes` (10 GiB default) → `413`.
   - Checks free space ≥ size + other unfinished uploads into that share + 1 GiB → `507`.
   - Checks the open-upload limits: 4 per device, 32 total → `429`.
2. A row goes into `uploads` (SQLite), and an empty partial file is created at full size in the target folder: `.riftview-upload-<id>.part`. It's on the same disk, so finishing is a rename.
3. **Chunks** (`PUT …/chunks/{index}`):
   - Index and exact length are checked.
   - The body's SHA-256 must match `X-Chunk-SHA256`, otherwise `422 checksum_mismatch`, and the device resends.
   - A good chunk is written at its offset and recorded in `upload_chunks`.
   - Resending a received chunk is harmless and isn't rewritten.
   - Each chunk holds a transfer slot (shared with downloads) and has its own 5-minute timeout.
4. **Complete:**
   - Needs every chunk (`409` otherwise).
   - The whole partial file is hashed and compared with the SHA-256 from the start. A mismatch discards the upload → `422`.
   - The data is flushed to disk, then the file is moved into place with a **hard link** to the wanted name, which can never replace an existing file. If the name is taken, it tries `name (1).ext`, `name (2).ext`… On disks without hard links (e.g. exFAT), it falls back to check-then-rename.
   - The partial file and rows are removed.
   - Only one finish (or cancel) can run per upload at a time. Chunks are refused while it does.
5. **Hidden while partial:** partial files are skipped in listings and can't be opened via the API, even with a forged file ID.
6. **Cleanup:** cancel, a failed final check, a share being removed, or **24 h with no new chunk** (swept at startup and every 10 minutes) deletes the partial file and rows.

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

**As built:**
- The manifest is the `uploads` row plus its `upload_chunks` rows (see `data_planning.md`): ID, device, share, target folder, name, size, chunk size, final SHA-256, created and last-activity times.
- Per-chunk hashes are checked on arrival rather than stored.
- Expiration is last activity + `upload_expiry_secs`.
- Status responses list `missing_chunks`, so the client resends exactly those.

## 8.5 Integrity Verification

Use SHA-256 or another established cryptographic hash.

Verify:
- Individual chunks where applicable.
- The final assembled file.
- Expected file size.

For downloads, account for the possibility that the source file changes during a transfer. Use a stable file handle and a clearly defined source-consistency strategy.

**As built (downloads):** `GET /api/files/{id}/hash` returns the SHA-256 of the whole file, plus its size and ETag. Hashing streams the file with a 1 MiB buffer and stops if the request is abandoned. It is exempt from the request timeout. Results are cached in memory (up to 10,000, least recently used evicted) until the file's version changes. If the file changes during hashing, the request returns `409`. The download stream and ETag above are the source-consistency strategy.

**As built (uploads):** every chunk is checked against its `X-Chunk-SHA256` before it is written, and the whole file against the SHA-256 given at the start before it becomes a real file. The size is fixed at the start and every chunk's length is checked. Finishing uses a hard link (atomic, never replaces an existing file), with a check-then-rename fallback on file systems without hard links.

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
| Concurrent transfers, total | Built: `max_concurrent_transfers`, 16 (downloads, hashes, upload chunks, finishing uploads) |
| Concurrent transfers, per device | Built: `max_transfers_per_device`, 4 (same) |
| Request timeouts | Built: 30 s until the response starts (`request_timeout_secs`); upload chunks 5 min (`chunk_upload_timeout_secs`); `/hash` and upload `complete` exempt; streaming isn't cut off |
| Bounded buffering | Built: 64 KiB download chunks, 1 MiB hash buffer, one 4 MiB chunk in memory per upload request |
| Request body size | Built: 64 KiB (`max_request_body_bytes`); upload chunks exactly their chunk length |
| Maximum upload size | Built: `max_upload_bytes`, 10 GiB, plus a free-space check (size + 1 GiB) |
| Maximum chunk size | Built: fixed 4 MiB |
| Active (unfinished) uploads | Built: `max_uploads_per_device` 4, `max_uploads_total` 32 |
| Transfer expiration | Built: `upload_expiry_secs`, 24 h after the last chunk |

Over-limit requests get `429 too_many_requests`.

---

# 9. Persistent State

No database server is required. Relational data lives in an embedded SQLite database (`riftview.db`), and a few small settings files sit beside it. Column-by-column details are in [data_planning.md](data_planning.md).

Data folder (`~/Library/Application Support/RiftView/` on macOS, or `RIFTVIEW_DATA_DIR`):

```text
RiftView/
|
├── riftview.db           SQLite: shares, devices, device_permissions, uploads, move_jobs
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
| `uploads` | Unfinished uploads: device, share, target folder, name, size, chunk size, expected SHA-256, activity times |
| `upload_chunks` | One row per received, verified chunk |
| `move_jobs` | Moves between disks: device, source, destination, state, error/warning, result; kept a day after finishing |

Persist:
- Host configuration (`config.json`).
- Shared directory definitions (SQLite).
- Trusted device public keys (SQLite).
- Device permissions (SQLite).
- Transfer metadata and resumable transfer state (SQLite, `uploads` + `upload_chunks`). Partial data lives in hidden `.part` files in the target folders.

Not persisted: login sessions, pairing sessions and the SHA-256 cache. They live in memory, so a restart logs devices out and they sign in again.

Use:
- Numbered schema migrations, tracked with `PRAGMA user_version`. Never edit a shipped migration; add a new one.
- SQLite transactions and WAL mode for safe recovery after unexpected termination.
- Atomic writes for the settings files.
- Versioned configuration schemas.
- Owner-only file permissions on the whole data folder.

Never persist private device keys or plaintext passwords. The host only stores device *public* keys; private keys stay on each device.

The backend should validate persisted transfer metadata against actual files before resuming operations.

**As built:**
- On restart, uploads continue from the chunks recorded in SQLite.
- A chunk is recorded only after its data is written.
- If the host crashes before that data reaches the disk, the whole-file SHA-256 check at finish catches it, and the upload is discarded.
- A missing partial file makes chunk writes and finishing fail with `404`, and the upload then expires.

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
- In memory: which uploads are being finished; progress and stop signals of running move jobs; SHA-256 results (invalidated by file version), shares and devices (loaded from SQLite at startup, written through on change), sessions and pairing state.
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

**As built:** 132 tests (`cargo test`).
- **Unit tests:** path parsing, Range parsing, ETags, hash cache, transfer limits, stop signal, migrations, crypto, dates, chunk arithmetic, rename candidates, reserved names, no-replace rename, real paths, tree copy/removal, AppleDouble skipping, CLI definitions.
- **`tests/api_tests.rs`:** browsing, metadata, downloads, resume, hashing, a file changing mid-download, limits, a 5 GiB sparse file.
- **`tests/auth_tests.rs`:** pairing and attack cases, login, replay, expiry, revocation, persistence across restart.
- **`tests/security_tests.rs`:** traversal, symlink escapes, named pipes, forged IDs.
- **`tests/upload_tests.rs`:** out-of-order chunks, corrupt and wrong-size chunks, whole-file mismatch, name clashes, permissions and read-only shares, size limit, disk space, open-upload limits, per-device privacy, hidden partial files, restart, expiry, share removal.
- **`tests/manage_tests.rs`:** rename (clash, case-only change, folders, bad names), move (within and between shares, clashes, loops, read-only destination), new folder, delete (files, whole folders, symlinks), permissions, read-only shares, and move jobs (copy, cancel, symlink refusal, privacy, restart recovery, busy shares).
- **Checked by hand against an exFAT disk image** (not in the suite):
  - moves between disks in both directions
  - rename clashes
  - the upload hard-link fallback
  - delete to the volume's Trash
  - `._` companion files being skipped

Not yet covered:
- config loading tests
- a race test for swapping a folder for a symlink mid-lookup
- automated tests for moves between disks and exFAT behaviour (they need a second disk; checked by hand, see above)
- the permanent-delete path on a real disk without a Trash (tests simulate it; the macOS "feature unsupported" code isn't checked against such a disk)
- that a delete lands in `~/.Trash` on the startup disk. macOS privacy rules stop the test from reading it, but a delete on an exFAT image was checked to land in that disk's `.Trashes`.
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

**Status: done.**
- Chunked (4 MiB), resumable, per-chunk and whole-file SHA-256, transfer state in SQLite.
- Atomic no-replace finish, with auto-rename on name clashes.
- 24 h expiry, size, disk-space and open-upload limits, "list my uploads".
- Not built: reading a chunk back (`GET …/chunks/{index}`). Remaining: testing the hard-link fallback on exFAT.

## Phase 7: Filesystem Management

Tasks:
- Implement rename.
- Implement move.
- Implement directory creation.
- Implement deletion.
- Enforce operation-specific permissions.
- Handle conflicts and partial failures.

Deliverable: Complete authorized filesystem management.

**Status: done.**
- Rename, move (within a share, between shares on the same disk, and between disks as a background job), new folder, delete. **Deletes always go to the Trash of the item's disk.** They're permanent only on a disk that has no Trash (macOS reports "feature unsupported"). Any other Trash failure deletes nothing (`409`). There is no setting to turn the Trash off.
- Each operation needs its own permission plus writable shares.
- Conflicts: rename, move and new folder never replace anything (`409`).
- Partial failures: a move between disks copies under a hidden name and only deletes the original once the copy has its real name. A failure or cancel removes the copy and leaves the original. A restart marks interrupted jobs failed and removes their copies.

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