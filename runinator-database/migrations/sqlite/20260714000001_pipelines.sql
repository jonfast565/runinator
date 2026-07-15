CREATE TABLE IF NOT EXISTS pipelines (
    id BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NULL,
    workflow_ids TEXT NOT NULL DEFAULT '[]',
    defaults TEXT NOT NULL DEFAULT '{}',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
