# Rust Backend Engineering Specification
## Self-Hosted Cross-Platform File Access Service

**Project:** Open-Source File Sharing Application  
**Language:** Rust  
**HTTP Framework:** Axum  
**Async Runtime:** Tokio  
**Serialization:** Serde  
**TLS:** Rustls  
**Target Platforms:** Windows, macOS, Linux  
**Deployment:** Native background service  
**Database:** None  
**External Infrastructure:** None required

---

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

```text
                 HTTP CLIENT
                      |
                      |
                   HTTPS
                      |
                      v
             +----------------+
             |  Axum Server   |
             +----------------+
                      |
                      v
             +----------------+
             | Middleware     |
             | - Authentication
             | - Rate Limiting |
             | - Request Limits|
             +----------------+
                      |
                      v
             +----------------+
             | Authorization  |
             | Permission      |
             | Enforcement     |
             +----------------+
                      |
           +----------+----------+
           |          |          |
           v          v          v
      +---------+ +---------+ +----------+
      | Filesystem| |Transfers| | Devices  |
      | Module    | | Module  | | Module   |
      +---------+ +---------+ +----------+
           |          |          |
           +----------+----------+
                      |
                      v
             +----------------+
             | Operating      |
             | System         |
             | Filesystem     |
             +----------------+
```

## 3.2 Module Responsibilities

### API Module

Responsible for:
- HTTP routing.
- Request validation.
- Response serialization.
- HTTP status codes.
- Error translation.
- Request cancellation.

### Authentication Module

Responsible for:
- Device pairing.
- Public-key identity management.
- Credential verification.
- Session management.
- Device revocation.

### Authorization Module

Responsible for:
- Shared-root permissions.
- Device-specific access rules.
- Read and write permissions.
- Destructive operation permissions.
- Transfer authorization.

### Filesystem Module

Responsible for:
- Directory browsing.
- File metadata.
- Safe path resolution.
- File opening.
- File creation.
- Rename and move operations.
- Deletion.
- Directory creation.

### Transfer Module

Responsible for:
- Transfer initialization.
- Chunk management.
- File streaming.
- Transfer progress.
- Integrity verification.
- Resumability.
- Temporary file management.
- Transfer cancellation.

### Platform Module

Responsible for:
- OS-specific behavior.
- Service installation.
- Startup integration.
- Platform-specific filesystem permissions.
- Application data directories.

---

# 4. Recommended Project Structure

```text
file-host/
|
├── Cargo.toml
├── Cargo.lock
|
├── src/
|   ├── main.rs
|   ├── server.rs
|   ├── config.rs
|   ├── state.rs
|   ├── error.rs
|   |
|   ├── api/
|   |   ├── mod.rs
|   |   ├── health.rs
|   |   ├── files.rs
|   |   ├── shares.rs
|   |   ├── devices.rs
|   |   └── transfers.rs
|   |
|   ├── auth/
|   |   ├── mod.rs
|   |   ├── pairing.rs
|   |   ├── credentials.rs
|   |   ├── sessions.rs
|   |   └── verification.rs
|   |
|   ├── authorization/
|   |   ├── mod.rs
|   |   ├── permissions.rs
|   |   └── policies.rs
|   |
|   ├── filesystem/
|   |   ├── mod.rs
|   |   ├── browser.rs
|   |   ├── resolver.rs
|   |   ├── metadata.rs
|   |   ├── operations.rs
|   |   └── roots.rs
|   |
|   ├── transfers/
|   |   ├── mod.rs
|   |   ├── manager.rs
|   |   ├── upload.rs
|   |   ├── download.rs
|   |   ├── chunks.rs
|   |   └── persistence.rs
|   |
|   ├── storage/
|   |   ├── mod.rs
|   |   ├── config_store.rs
|   |   └── state_store.rs
|   |
|   └── platform/
|       ├── mod.rs
|       ├── windows.rs
|       ├── macos.rs
|       └── linux.rs
|
└── tests/
    ├── api_tests.rs
    ├── filesystem_tests.rs
    ├── security_tests.rs
    ├── auth_tests.rs
    └── transfer_tests.rs
```

The exact module boundaries can evolve during development. Avoid introducing abstractions before they provide a practical benefit.

---

# 5. HTTP API Design

The backend exposes a REST API over HTTPS.

All endpoints must authenticate the requesting device and enforce the appropriate authorization policy.

## 5.1 Health and Host Information

| Method | Endpoint | Purpose |
|---|---|---|
| GET | `/api/health` | Health check |
| GET | `/api/host` | Retrieve host information |
| GET | `/api/version` | Retrieve backend version |

The health endpoint should return minimal information and must not expose sensitive host details to unauthenticated clients.

## 5.2 Device Management

| Method | Endpoint | Purpose |
|---|---|---|
| POST | `/api/pairing/initiate` | Initiate pairing |
| POST | `/api/pairing/complete` | Complete pairing |
| GET | `/api/devices` | List trusted devices |
| GET | `/api/devices/{id}` | Retrieve device information |
| DELETE | `/api/devices/{id}` | Revoke device access |

## 5.3 Shared Directories

| Method | Endpoint | Purpose |
|---|---|---|
| GET | `/api/shares` | List shared roots |
| POST | `/api/shares` | Register a shared root |
| GET | `/api/shares/{id}` | Retrieve share information |
| PATCH | `/api/shares/{id}` | Modify share permissions |
| DELETE | `/api/shares/{id}` | Remove a shared root |

## 5.4 Filesystem Operations

| Method | Endpoint | Purpose |
|---|---|---|
| GET | `/api/shares/{id}/files` | Browse a directory |
| GET | `/api/files/{id}` | Retrieve file metadata |
| GET | `/api/files/{id}/download` | Download a file |
| POST | `/api/files/{id}/rename` | Rename a file |
| POST | `/api/files/{id}/move` | Move a file |
| POST | `/api/shares/{id}/directories` | Create a directory |
| DELETE | `/api/files/{id}` | Delete a file |

File identifiers should resolve to resources within authorized shared roots. Avoid treating arbitrary client-supplied absolute paths as valid resource identifiers.

## 5.5 Transfers

| Method | Endpoint | Purpose |
|---|---|---|
| POST | `/api/transfers` | Initialize transfer |
| GET | `/api/transfers/{id}` | Retrieve transfer status |
| PUT | `/api/transfers/{id}/chunks/{index}` | Upload chunk |
| GET | `/api/transfers/{id}/chunks/{index}` | Retrieve chunk |
| POST | `/api/transfers/{id}/complete` | Finalize transfer |
| DELETE | `/api/transfers/{id}` | Cancel transfer |

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

## 7.3 Session Management

The backend may issue short-lived sessions after verifying device identity.

Sessions should:
- Expire.
- Be revocable.
- Be bound to the authenticated device.
- Carry no more authority than the device's permissions.
- Be protected against replay.

## 7.4 Authorization

Authentication and authorization are separate responsibilities.

A successfully authenticated device must not automatically have unrestricted access.

Example permission model:

| Permission | Description |
|---|---|
| Browse | List authorized directories |
| Read | Download files |
| Upload | Create new files |
| Rename | Rename files |
| Move | Move files |
| Delete | Delete files |
| Manage Shares | Modify shared roots |

Permissions should be enforced by the backend for every operation.

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

---

# 9. Persistent State

No database server is required.

The backend can maintain small local configuration and state files.

Example:

```text
application-data/
|
├── config.json
├── shared_directories.json
├── trusted_devices.json
|
└── transfers/
    ├── transfer_001.json
    └── transfer_002.json
```

Persist:
- Host configuration.
- Shared directory definitions.
- Trusted device public keys.
- Device permissions.
- Transfer metadata.
- Resumable transfer state.

Use:
- Atomic writes.
- Versioned configuration schemas.
- Appropriate file permissions.
- Safe recovery after unexpected termination.

Never persist private device keys or plaintext passwords in ordinary configuration files.

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

## Phase 3: REST API

Tasks:
- Implement filesystem routes.
- Add request validation.
- Add JSON serialization.
- Add structured errors.
- Implement pagination or bounded directory listings.
- Add API integration tests.

Deliverable: A functional filesystem REST API.

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

## Phase 5: Download Engine

Tasks:
- Implement streaming downloads.
- Implement bounded buffering.
- Implement HTTP range support.
- Implement transfer cancellation.
- Add integrity verification.
- Test large files.

Deliverable: Reliable large-file downloads.

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

## Phase 7: Filesystem Management

Tasks:
- Implement rename.
- Implement move.
- Implement directory creation.
- Implement deletion.
- Enforce operation-specific permissions.
- Handle conflicts and partial failures.

Deliverable: Complete authorized filesystem management.

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
| TLS | Rustls |
| Logging | Tracing |
| Identifiers | UUID |
| Integrity | SHA-256 |
| Filesystem | std::fs / tokio::fs |
| Persistence | Local files |
| Cache | In-process, if needed |
| Database | None |
| Deployment | Native background process |

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