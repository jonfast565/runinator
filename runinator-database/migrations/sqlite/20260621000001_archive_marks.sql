CREATE TABLE IF NOT EXISTS archive_marks (
    id BLOB PRIMARY KEY,
    table_name TEXT NOT NULL,
    primary_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    eligible_before INTEGER NOT NULL,
    archive_day TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'marked',
    claimed_by TEXT NULL,
    claimed_until INTEGER NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NULL,
    marked_at INTEGER NOT NULL,
    archived_at INTEGER NULL,
    UNIQUE(table_name, primary_key)
);

CREATE INDEX IF NOT EXISTS idx_archive_marks_claim ON archive_marks(status, archive_day, claimed_until);
CREATE INDEX IF NOT EXISTS idx_archive_marks_table_status ON archive_marks(table_name, status);
