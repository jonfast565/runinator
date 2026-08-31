ALTER TABLE pipeline_runs ADD COLUMN orchestration_binding_id UUID NULL;
ALTER TABLE pipeline_runs ADD COLUMN execution_epoch BIGINT NULL;
ALTER TABLE pipeline_runs ADD COLUMN start_member TEXT NULL;
CREATE UNIQUE INDEX idx_pipeline_runs_orchestration
    ON pipeline_runs(orchestration_binding_id, execution_epoch);

CREATE TABLE orchestration_bindings (
    id UUID PRIMARY KEY, admission_id UUID NOT NULL REFERENCES ingress_admissions(id) ON DELETE CASCADE,
    generation BIGINT NOT NULL, pipeline_revision BIGINT NOT NULL,
    pipeline_digest TEXT NOT NULL, policy TEXT NOT NULL, status TEXT NOT NULL, current_phase TEXT NULL,
    current_attempt BIGINT NOT NULL DEFAULT 0, current_epoch BIGINT NOT NULL DEFAULT 0,
    restart_member TEXT NULL, resume_existing_epoch BOOLEAN NOT NULL DEFAULT FALSE,
    subject_revision TEXT NULL, resources TEXT NOT NULL DEFAULT 'null', budgets TEXT NOT NULL DEFAULT '{}',
    last_reduced_sequence BIGINT NOT NULL DEFAULT 0, version BIGINT NOT NULL DEFAULT 0,
    reducer_lease_owner TEXT NULL, reducer_leased_until BIGINT NULL,
    created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, finished_at BIGINT NULL,
    UNIQUE(admission_id, generation)
);
CREATE INDEX idx_orchestration_bindings_claim ON orchestration_bindings(status, reducer_leased_until, updated_at);

CREATE TABLE orchestration_epochs (
    id UUID PRIMARY KEY, binding_id UUID NOT NULL REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    epoch BIGINT NOT NULL, pipeline_run_id UUID NULL REFERENCES pipeline_runs(id), start_member TEXT NULL,
    parameters TEXT NOT NULL, status TEXT NOT NULL, reason TEXT NOT NULL, created_at BIGINT NOT NULL,
    started_at BIGINT NULL, finished_at BIGINT NULL, UNIQUE(binding_id, epoch), UNIQUE(pipeline_run_id)
);
CREATE TABLE orchestration_event_reductions (
    id UUID PRIMARY KEY, binding_id UUID NOT NULL REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    inbox_event_id UUID NOT NULL REFERENCES ingress_events(id) ON DELETE CASCADE, sequence BIGINT NOT NULL,
    matched_intents TEXT NOT NULL, winner TEXT NULL, suppressed_intents TEXT NOT NULL,
    binding_version BIGINT NOT NULL, disposition TEXT NOT NULL, detail TEXT NOT NULL, created_at BIGINT NOT NULL,
    UNIQUE(inbox_event_id), UNIQUE(binding_id, sequence)
);
CREATE TABLE orchestration_pending_intents (
    id UUID PRIMARY KEY, binding_id UUID NOT NULL REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    intent TEXT NOT NULL, priority BIGINT NOT NULL, source_event_ids TEXT NOT NULL, latest_payload TEXT NOT NULL,
    wake_at BIGINT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, UNIQUE(binding_id, intent)
);
CREATE INDEX idx_orchestration_pending_wake ON orchestration_pending_intents(wake_at);
CREATE TABLE orchestration_commands (
    id UUID PRIMARY KEY, binding_id UUID NOT NULL REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    epoch BIGINT NOT NULL, command_type TEXT NOT NULL, operation_key TEXT NOT NULL, payload TEXT NOT NULL,
    status TEXT NOT NULL, attempts BIGINT NOT NULL DEFAULT 0, claimed_by TEXT NULL, claimed_until BIGINT NULL,
    result TEXT NOT NULL DEFAULT 'null', created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
    UNIQUE(binding_id, operation_key)
);
CREATE INDEX idx_orchestration_commands_claim ON orchestration_commands(status, claimed_until, created_at);
CREATE TABLE orchestration_evidence (
    id UUID PRIMARY KEY, binding_id UUID NOT NULL REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    epoch BIGINT NULL, kind TEXT NOT NULL, subject_revision TEXT NULL, payload TEXT NOT NULL,
    source_event_id UUID NULL REFERENCES ingress_events(id) ON DELETE SET NULL, created_at BIGINT NOT NULL
);
CREATE INDEX idx_orchestration_evidence_binding ON orchestration_evidence(binding_id, created_at);
CREATE TABLE orchestration_adapters (
    id UUID PRIMARY KEY, org_id UUID NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL,
    current_revision BIGINT NOT NULL, enabled BOOLEAN NOT NULL, endpoint_identity TEXT NOT NULL,
    has_admitted_binding BOOLEAN NOT NULL DEFAULT FALSE, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
    UNIQUE(org_id, name), UNIQUE(endpoint_identity)
);
CREATE TABLE orchestration_adapter_revisions (
    id UUID PRIMARY KEY, adapter_id UUID NOT NULL REFERENCES orchestration_adapters(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL, kind_version TEXT NOT NULL, configuration TEXT NOT NULL,
    secret_bindings TEXT NOT NULL, identity_configuration TEXT NOT NULL, created_at BIGINT NOT NULL,
    actor_id UUID NULL, UNIQUE(adapter_id, revision)
);
CREATE TABLE external_operations (
    id UUID PRIMARY KEY, binding_id UUID NOT NULL REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    operation_key TEXT NOT NULL, provider TEXT NOT NULL, action TEXT NOT NULL, semantics TEXT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0, status TEXT NOT NULL, ambiguous BOOLEAN NOT NULL DEFAULT FALSE,
    provenance TEXT NOT NULL, receipt TEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
    UNIQUE(binding_id, operation_key)
);
CREATE INDEX idx_external_operations_status ON external_operations(binding_id, status, updated_at);
