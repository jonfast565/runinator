CREATE TABLE IF NOT EXISTS archive_marks (
    id BINARY(16) PRIMARY KEY,
    table_name TEXT NOT NULL,
    primary_key VARCHAR(255) NOT NULL,
    created_at BIGINT NOT NULL,
    eligible_before BIGINT NOT NULL,
    archive_day TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT ('marked'),
    claimed_by VARCHAR(255) NULL,
    claimed_until BIGINT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    last_error TEXT NULL,
    marked_at BIGINT NOT NULL,
    archived_at BIGINT NULL,
    UNIQUE(table_name(191), primary_key)
);

-- every TEXT column in a key needs a prefix length: mysql otherwise indexes the declared maximum
-- and rejects the key as too long, which used to fail this migration and every one after it.
CREATE INDEX idx_archive_marks_claim ON archive_marks(status(32), archive_day(32), claimed_until);
CREATE INDEX idx_archive_marks_table_status ON archive_marks(table_name(191), status(32));
