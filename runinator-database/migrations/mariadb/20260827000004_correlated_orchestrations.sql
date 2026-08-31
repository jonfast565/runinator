-- Index widths here are bounded by mysql's 3072-byte limit (four bytes per utf8mb4 character).
-- Note which indexes may carry a column prefix and which may not: a prefix on a *unique* key is a
-- weaker constraint, not an optimization, so two distinct values sharing the prefix would collide
-- and be falsely rejected -- while sqlite and postgres constrain the whole value. Every unique key
-- below is therefore exact. A prefix on a plain lookup index is fine, since it only narrows the
-- rows the engine then re-checks.
ALTER TABLE pipeline_runs ADD COLUMN orchestration_binding_id BINARY(16) NULL;
ALTER TABLE pipeline_runs ADD COLUMN execution_epoch BIGINT NULL;
ALTER TABLE pipeline_runs ADD COLUMN start_member VARCHAR(255) NULL;
CREATE UNIQUE INDEX idx_pipeline_runs_orchestration ON pipeline_runs(orchestration_binding_id, execution_epoch);

CREATE TABLE orchestration_bindings (
    id BINARY(16) PRIMARY KEY, admission_id BINARY(16) NOT NULL, generation BIGINT NOT NULL,
    pipeline_revision BIGINT NOT NULL, pipeline_digest VARCHAR(80) NOT NULL,
    policy LONGTEXT NOT NULL, status VARCHAR(32) NOT NULL, current_phase VARCHAR(255) NULL,
    current_attempt BIGINT NOT NULL DEFAULT 0, current_epoch BIGINT NOT NULL DEFAULT 0,
    restart_member VARCHAR(255) NULL, resume_existing_epoch BOOLEAN NOT NULL DEFAULT FALSE,
    subject_revision VARCHAR(512) NULL, resources LONGTEXT NOT NULL, budgets LONGTEXT NOT NULL,
    last_reduced_sequence BIGINT NOT NULL DEFAULT 0, version BIGINT NOT NULL DEFAULT 0,
    reducer_lease_owner VARCHAR(255) NULL, reducer_leased_until BIGINT NULL,
    created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, finished_at BIGINT NULL,
    CONSTRAINT fk_orchestration_admission FOREIGN KEY (admission_id) REFERENCES ingress_admissions(id) ON DELETE CASCADE,
    UNIQUE KEY idx_orchestration_generation (admission_id, generation),
    KEY idx_orchestration_bindings_claim (status, reducer_leased_until, updated_at)
);
CREATE TABLE orchestration_epochs (
    id BINARY(16) PRIMARY KEY, binding_id BINARY(16) NOT NULL, epoch BIGINT NOT NULL,
    pipeline_run_id BINARY(16) NULL, start_member VARCHAR(255) NULL, parameters LONGTEXT NOT NULL,
    status VARCHAR(32) NOT NULL, reason TEXT NOT NULL, created_at BIGINT NOT NULL,
    started_at BIGINT NULL, finished_at BIGINT NULL,
    CONSTRAINT fk_orchestration_epoch_binding FOREIGN KEY (binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    CONSTRAINT fk_orchestration_epoch_run FOREIGN KEY (pipeline_run_id) REFERENCES pipeline_runs(id),
    UNIQUE KEY idx_orchestration_epoch (binding_id, epoch), UNIQUE KEY idx_orchestration_epoch_run (pipeline_run_id)
);
CREATE TABLE orchestration_event_reductions (
    id BINARY(16) PRIMARY KEY, binding_id BINARY(16) NOT NULL, inbox_event_id BINARY(16) NOT NULL,
    sequence BIGINT NOT NULL, matched_intents LONGTEXT NOT NULL, winner VARCHAR(255) NULL,
    suppressed_intents LONGTEXT NOT NULL, binding_version BIGINT NOT NULL, disposition VARCHAR(64) NOT NULL,
    detail LONGTEXT NOT NULL, created_at BIGINT NOT NULL,
    CONSTRAINT fk_orchestration_reduction_binding FOREIGN KEY (binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    CONSTRAINT fk_orchestration_reduction_event FOREIGN KEY (inbox_event_id) REFERENCES ingress_events(id) ON DELETE CASCADE,
    UNIQUE KEY idx_orchestration_reduction_event (inbox_event_id),
    UNIQUE KEY idx_orchestration_reduction_sequence (binding_id, sequence)
);
CREATE TABLE orchestration_pending_intents (
    id BINARY(16) PRIMARY KEY, binding_id BINARY(16) NOT NULL, intent VARCHAR(255) NOT NULL,
    priority BIGINT NOT NULL, source_event_ids LONGTEXT NOT NULL, latest_payload LONGTEXT NOT NULL,
    wake_at BIGINT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
    CONSTRAINT fk_orchestration_pending_binding FOREIGN KEY (binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    UNIQUE KEY idx_orchestration_pending_intent (binding_id, intent), KEY idx_orchestration_pending_wake (wake_at)
);
CREATE TABLE orchestration_commands (
    id BINARY(16) PRIMARY KEY, binding_id BINARY(16) NOT NULL, epoch BIGINT NOT NULL,
    command_type VARCHAR(64) NOT NULL, operation_key VARCHAR(512) NOT NULL, payload LONGTEXT NOT NULL,
    status VARCHAR(32) NOT NULL, attempts BIGINT NOT NULL DEFAULT 0, claimed_by VARCHAR(255) NULL,
    claimed_until BIGINT NULL, result LONGTEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
    CONSTRAINT fk_orchestration_command_binding FOREIGN KEY (binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    UNIQUE KEY idx_orchestration_command_key (binding_id, operation_key),
    KEY idx_orchestration_commands_claim (status, claimed_until, created_at)
);
CREATE TABLE orchestration_evidence (
    id BINARY(16) PRIMARY KEY, binding_id BINARY(16) NOT NULL, epoch BIGINT NULL,
    kind VARCHAR(255) NOT NULL, subject_revision VARCHAR(512) NULL, payload LONGTEXT NOT NULL,
    source_event_id BINARY(16) NULL, created_at BIGINT NOT NULL,
    CONSTRAINT fk_orchestration_evidence_binding FOREIGN KEY (binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    CONSTRAINT fk_orchestration_evidence_event FOREIGN KEY (source_event_id) REFERENCES ingress_events(id) ON DELETE SET NULL,
    KEY idx_orchestration_evidence_binding (binding_id, created_at)
);
CREATE TABLE orchestration_adapters (
    id BINARY(16) PRIMARY KEY, org_id BINARY(16) NOT NULL, name VARCHAR(255) NOT NULL,
    kind VARCHAR(255) NOT NULL, current_revision BIGINT NOT NULL, enabled BOOLEAN NOT NULL,
    endpoint_identity VARCHAR(255) NOT NULL, has_admitted_binding BOOLEAN NOT NULL DEFAULT FALSE,
    created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
    UNIQUE KEY idx_orchestration_adapter_name (org_id, name),
    UNIQUE KEY idx_orchestration_adapter_endpoint (endpoint_identity)
);
CREATE TABLE orchestration_adapter_revisions (
    id BINARY(16) PRIMARY KEY, adapter_id BINARY(16) NOT NULL, revision BIGINT NOT NULL,
    kind_version VARCHAR(255) NOT NULL, configuration LONGTEXT NOT NULL, secret_bindings LONGTEXT NOT NULL,
    identity_configuration LONGTEXT NOT NULL, created_at BIGINT NOT NULL, actor_id BINARY(16) NULL,
    CONSTRAINT fk_orchestration_adapter_revision FOREIGN KEY (adapter_id) REFERENCES orchestration_adapters(id) ON DELETE CASCADE,
    UNIQUE KEY idx_orchestration_adapter_revision (adapter_id, revision)
);
CREATE TABLE external_operations (
    id BINARY(16) PRIMARY KEY, binding_id BINARY(16) NOT NULL, operation_key VARCHAR(512) NOT NULL,
    provider VARCHAR(255) NOT NULL, action VARCHAR(255) NOT NULL, semantics VARCHAR(32) NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0, status VARCHAR(32) NOT NULL, ambiguous BOOLEAN NOT NULL DEFAULT FALSE,
    provenance LONGTEXT NOT NULL, receipt LONGTEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
    CONSTRAINT fk_external_operation_binding FOREIGN KEY (binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    UNIQUE KEY idx_external_operation_key (binding_id, operation_key),
    KEY idx_external_operations_status (binding_id, status, updated_at)
);
