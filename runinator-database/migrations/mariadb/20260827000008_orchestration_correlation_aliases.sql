CREATE TABLE IF NOT EXISTS orchestration_correlation_aliases (
    id BINARY(16) PRIMARY KEY,
    binding_id BINARY(16) NOT NULL,
    generation BIGINT NOT NULL,
    org_scope VARCHAR(36) NOT NULL,
    source VARCHAR(128) NOT NULL,
    scope VARCHAR(255) NOT NULL,
    correlation_key VARCHAR(255) NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_orchestration_alias_binding FOREIGN KEY(binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    UNIQUE KEY uq_orchestration_alias_identity(org_scope, source, scope, correlation_key),
    KEY idx_orchestration_aliases_binding(binding_id, created_at)
);
