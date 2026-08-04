-- a scheduled suspension of trigger firing. a window with no workflow_id freezes every workflow in
-- its org (and every pipeline cron trigger); one with no org_id freezes the whole platform.
CREATE TABLE IF NOT EXISTS freeze_windows (
    id BINARY(16) PRIMARY KEY,
    org_id BINARY(16) NULL,
    workflow_id BINARY(16) NULL,
    name TEXT NOT NULL,
    reason TEXT NULL,
    starts_at BIGINT NOT NULL,
    ends_at BIGINT NOT NULL,
    enabled TINYINT(1) NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- the trigger loop's hot path is "is any window active right now", so lead on the bounds.
CREATE INDEX idx_freeze_windows_active
    ON freeze_windows(enabled, starts_at, ends_at);

-- why a due slot produced no run. 'fired' for every pre-policy row, so history stays readable.
ALTER TABLE workflow_trigger_firings ADD COLUMN outcome VARCHAR(32) NOT NULL DEFAULT 'fired';
