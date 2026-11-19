-- Background moves between shares on different disks (Phase 7).
--
-- Such a move copies the item into a hidden temporary copy in the
-- destination folder, gives the copy its real name, then deletes the
-- original. A row records each job so a device can follow it, and so that
-- after a host restart an interrupted copy can be found and removed.
-- Finished jobs are kept for a day, then deleted.

CREATE TABLE move_jobs (
    id                    TEXT    PRIMARY KEY,   -- e.g. "job_3f2a…"
    device_id             TEXT    NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    source_share_id       TEXT    NOT NULL,
    source_path           TEXT    NOT NULL,      -- the item being moved, e.g. "/Photos/2019"
    destination_share_id  TEXT    NOT NULL,
    destination_folder    TEXT    NOT NULL,      -- where it goes, e.g. "/Archive"
    state                 TEXT    NOT NULL CHECK (state IN ('running', 'done', 'failed', 'cancelled')),
    error                 TEXT,                  -- why it failed (state = 'failed')
    warning               TEXT,                  -- done, but e.g. the original couldn't be fully deleted
    result_json           TEXT,                  -- the moved item as JSON (state = 'done')
    created_at_ms         INTEGER NOT NULL,
    finished_at_ms        INTEGER
) STRICT;

CREATE INDEX move_jobs_by_state ON move_jobs (state);
