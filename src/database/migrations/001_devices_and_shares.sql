-- First schema: shares, trusted devices, and what each device may do.
--
-- Times are Unix epoch milliseconds. Tables are STRICT, so SQLite refuses
-- values of the wrong type instead of silently storing them.

-- Folders the host user chose to expose.
CREATE TABLE shares (
    id            TEXT    PRIMARY KEY,           -- e.g. "shr_3f2a…"
    name          TEXT    NOT NULL,              -- shown to devices
    path          TEXT    NOT NULL UNIQUE,       -- real host path, never sent to devices
    writable      INTEGER NOT NULL CHECK (writable IN (0, 1)),
    created_at_ms INTEGER NOT NULL
) STRICT;

-- Devices that completed pairing and were approved by the host user.
CREATE TABLE devices (
    id                   TEXT    PRIMARY KEY,    -- e.g. "dev_3f2a…", also used by the app
    name                 TEXT    NOT NULL,       -- name the device gave itself
    custom_alias         TEXT,                   -- name the host user gave it, if any
    public_key           TEXT    NOT NULL,       -- Ed25519 public key, base64
    key_version          INTEGER NOT NULL,       -- bumped if the device ever replaces its key
    device_type          TEXT    NOT NULL,       -- phone, tablet, laptop, desktop, other
    platform             TEXT    NOT NULL,       -- ios, android, macos, windows, linux, other
    app_version          TEXT    NOT NULL,       -- e.g. "1.4.0", updated at each login
    paired_at_ms         INTEGER NOT NULL,
    last_connected_at_ms INTEGER,                -- last successful login
    revoked_at_ms        INTEGER                 -- set = revoked forever, row kept as a record
) STRICT;

-- One row per permission a device has. No row = not allowed.
CREATE TABLE device_permissions (
    device_id  TEXT NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    permission TEXT NOT NULL
        CHECK (permission IN ('browse', 'read', 'upload', 'rename', 'move', 'delete')),
    PRIMARY KEY (device_id, permission)
) STRICT;
