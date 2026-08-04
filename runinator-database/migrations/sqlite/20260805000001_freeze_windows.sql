-- a scheduled suspension of trigger firing. a window with no workflow_id freezes every workflow in
-- its org (and every pipeline cron trigger); one with no org_id freezes the whole platform.
CREATE TABLE IF NOT EXISTS freeze_windows (
    id BLOB PRIMARY KEY,
    org_id BLOB NULL,
    workflow_id BLOB NULL REFERENCES workflows(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    reason TEXT NULL,
    starts_at INTEGER NOT NULL,
    ends_at INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- the trigger loop's hot path is "is any window active right now", so lead on the bounds.
CREATE INDEX IF NOT EXISTS idx_freeze_windows_active
    ON freeze_windows(enabled, starts_at, ends_at);

-- why a due slot produced no run. 'fired' for every pre-policy row, so history stays readable.
ALTER TABLE workflow_trigger_firings ADD COLUMN outcome TEXT NOT NULL DEFAULT 'fired';
