-- Unfinished uploads (Phase 6).
--
-- A row exists only while an upload is in progress. Finishing, cancelling
-- or expiring an upload deletes its row (and its chunk rows with it).
-- The partial file itself lives in the target folder as a hidden
-- `.riftview-upload-<id>.part` file, so finishing is an atomic rename on
-- the same disk.

CREATE TABLE uploads (
    id                TEXT    PRIMARY KEY,   -- e.g. "upl_3f2a…"
    device_id         TEXT    NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    share_id          TEXT    NOT NULL REFERENCES shares (id) ON DELETE CASCADE,
    folder_path       TEXT    NOT NULL,      -- target folder inside the share, e.g. "/Photos"
    file_name         TEXT    NOT NULL,      -- name the device asked for
    size              INTEGER NOT NULL CHECK (size >= 0),
    chunk_size        INTEGER NOT NULL CHECK (chunk_size > 0),
    sha256            TEXT    NOT NULL,      -- expected SHA-256 of the whole file, lowercase hex
    created_at_ms     INTEGER NOT NULL,
    last_activity_ms  INTEGER NOT NULL       -- start or last chunk received; drives expiry
) STRICT;

CREATE INDEX uploads_by_device ON uploads (device_id);
CREATE INDEX uploads_by_share ON uploads (share_id);

-- One row per chunk that has been received and verified.
CREATE TABLE upload_chunks (
    upload_id    TEXT    NOT NULL REFERENCES uploads (id) ON DELETE CASCADE,
    chunk_index  INTEGER NOT NULL CHECK (chunk_index >= 0),
    PRIMARY KEY (upload_id, chunk_index)
) STRICT;
