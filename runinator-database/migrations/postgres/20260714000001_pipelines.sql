CREATE TABLE IF NOT EXISTS pipelines (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NULL,
    workflow_ids TEXT NOT NULL DEFAULT '[]',
    defaults TEXT NOT NULL DEFAULT '{}',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
