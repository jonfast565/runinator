CREATE TABLE IF NOT EXISTS pipeline_revisions (
    id BINARY(16) PRIMARY KEY,
    pipeline_id BINARY(16) NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL,
    digest VARCHAR(72) NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT NULL,
    graph LONGTEXT NOT NULL,
    concurrency LONGTEXT NOT NULL,
    defaults LONGTEXT NOT NULL,
    metadata LONGTEXT NOT NULL,
    source VARCHAR(32) NOT NULL DEFAULT 'api',
    actor_id BINARY(16) NULL,
    actor_kind VARCHAR(64) NOT NULL DEFAULT 'unknown',
    note TEXT NULL,
    created_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX idx_pipeline_revisions_seq
    ON pipeline_revisions(pipeline_id, revision);
CREATE INDEX idx_pipeline_revisions_created
    ON pipeline_revisions(pipeline_id, created_at);
