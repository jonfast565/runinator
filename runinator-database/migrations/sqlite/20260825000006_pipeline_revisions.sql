CREATE TABLE IF NOT EXISTS pipeline_revisions (
    id BLOB PRIMARY KEY,
    pipeline_id BLOB NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    digest TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NULL,
    graph TEXT NOT NULL,
    concurrency TEXT NOT NULL,
    defaults TEXT NOT NULL,
    metadata TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'api',
    actor_id BLOB NULL,
    actor_kind TEXT NOT NULL DEFAULT 'unknown',
    note TEXT NULL,
    created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_pipeline_revisions_seq
    ON pipeline_revisions(pipeline_id, revision);
CREATE INDEX IF NOT EXISTS idx_pipeline_revisions_created
    ON pipeline_revisions(pipeline_id, created_at);
