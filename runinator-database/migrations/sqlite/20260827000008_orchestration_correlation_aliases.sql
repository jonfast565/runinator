CREATE TABLE IF NOT EXISTS orchestration_correlation_aliases (
    id BLOB PRIMARY KEY,
    binding_id BLOB NOT NULL,
    generation INTEGER NOT NULL,
    org_scope TEXT NOT NULL,
    source TEXT NOT NULL,
    scope TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    UNIQUE(org_scope, source, scope, correlation_key)
);
CREATE INDEX IF NOT EXISTS idx_orchestration_aliases_binding
    ON orchestration_correlation_aliases(binding_id, created_at);
