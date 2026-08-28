CREATE TABLE IF NOT EXISTS pipelines (
    id BINARY(16) PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NULL,
    workflow_ids LONGTEXT NOT NULL,
    defaults LONGTEXT NOT NULL,
    metadata LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
