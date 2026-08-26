CREATE TABLE IF NOT EXISTS pipeline_revisions (
    id UUID PRIMARY KEY,
    pipeline_id UUID NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL,
    digest TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    graph TEXT NOT NULL,
    concurrency TEXT NOT NULL,
    defaults TEXT NOT NULL,
    metadata TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'api',
    actor_id UUID,
    actor_kind TEXT NOT NULL DEFAULT 'unknown',
    note TEXT,
    created_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_pipeline_revisions_seq
    ON pipeline_revisions(pipeline_id, revision);
CREATE INDEX IF NOT EXISTS idx_pipeline_revisions_created
    ON pipeline_revisions(pipeline_id, created_at);
