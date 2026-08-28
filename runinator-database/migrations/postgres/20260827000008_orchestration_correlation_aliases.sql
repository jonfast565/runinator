CREATE TABLE IF NOT EXISTS orchestration_correlation_aliases (
    id UUID PRIMARY KEY,
    binding_id UUID NOT NULL REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    generation BIGINT NOT NULL,
    org_scope TEXT NOT NULL,
    source TEXT NOT NULL,
    scope TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE(org_scope, source, scope, correlation_key)
);
CREATE INDEX IF NOT EXISTS idx_orchestration_aliases_binding
    ON orchestration_correlation_aliases(binding_id, created_at);
