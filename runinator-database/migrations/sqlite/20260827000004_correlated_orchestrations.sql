ALTER TABLE pipeline_runs ADD COLUMN orchestration_binding_id BLOB NULL;
ALTER TABLE pipeline_runs ADD COLUMN execution_epoch INTEGER NULL;
ALTER TABLE pipeline_runs ADD COLUMN start_member TEXT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_pipeline_runs_orchestration
    ON pipeline_runs(orchestration_binding_id, execution_epoch);

CREATE TABLE IF NOT EXISTS orchestration_bindings (
    id BLOB PRIMARY KEY,
    admission_id BLOB NOT NULL,
    generation INTEGER NOT NULL,
    pipeline_revision INTEGER NOT NULL,
    pipeline_digest TEXT NOT NULL,
    policy TEXT NOT NULL,
    status TEXT NOT NULL,
    current_phase TEXT NULL,
    current_attempt INTEGER NOT NULL DEFAULT 0,
    current_epoch INTEGER NOT NULL DEFAULT 0,
    restart_member TEXT NULL,
    resume_existing_epoch INTEGER NOT NULL DEFAULT 0,
    subject_revision TEXT NULL,
    resources TEXT NOT NULL DEFAULT 'null',
    budgets TEXT NOT NULL DEFAULT '{}',
    last_reduced_sequence INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 0,
    reducer_lease_owner TEXT NULL,
    reducer_leased_until INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    finished_at INTEGER NULL,
    FOREIGN KEY(admission_id) REFERENCES ingress_admissions(id) ON DELETE CASCADE,
    UNIQUE(admission_id, generation)
);
CREATE INDEX IF NOT EXISTS idx_orchestration_bindings_claim
    ON orchestration_bindings(status, reducer_leased_until, updated_at);

CREATE TABLE IF NOT EXISTS orchestration_epochs (
    id BLOB PRIMARY KEY,
    binding_id BLOB NOT NULL,
    epoch INTEGER NOT NULL,
    pipeline_run_id BLOB NULL,
    start_member TEXT NULL,
    parameters TEXT NOT NULL,
    status TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL,
    FOREIGN KEY(binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    FOREIGN KEY(pipeline_run_id) REFERENCES pipeline_runs(id),
    UNIQUE(binding_id, epoch),
    UNIQUE(pipeline_run_id)
);

CREATE TABLE IF NOT EXISTS orchestration_event_reductions (
    id BLOB PRIMARY KEY,
    binding_id BLOB NOT NULL,
    inbox_event_id BLOB NOT NULL,
    sequence INTEGER NOT NULL,
    matched_intents TEXT NOT NULL,
    winner TEXT NULL,
    suppressed_intents TEXT NOT NULL,
    binding_version INTEGER NOT NULL,
    disposition TEXT NOT NULL,
    detail TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    FOREIGN KEY(inbox_event_id) REFERENCES ingress_events(id) ON DELETE CASCADE,
    UNIQUE(inbox_event_id),
    UNIQUE(binding_id, sequence)
);

CREATE TABLE IF NOT EXISTS orchestration_pending_intents (
    id BLOB PRIMARY KEY,
    binding_id BLOB NOT NULL,
    intent TEXT NOT NULL,
    priority INTEGER NOT NULL,
    source_event_ids TEXT NOT NULL,
    latest_payload TEXT NOT NULL,
    wake_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    UNIQUE(binding_id, intent)
);
CREATE INDEX IF NOT EXISTS idx_orchestration_pending_wake
    ON orchestration_pending_intents(wake_at);

CREATE TABLE IF NOT EXISTS orchestration_commands (
    id BLOB PRIMARY KEY,
    binding_id BLOB NOT NULL,
    epoch INTEGER NOT NULL,
    command_type TEXT NOT NULL,
    operation_key TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    claimed_by TEXT NULL,
    claimed_until INTEGER NULL,
    result TEXT NOT NULL DEFAULT 'null',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    UNIQUE(binding_id, operation_key)
);
CREATE INDEX IF NOT EXISTS idx_orchestration_commands_claim
    ON orchestration_commands(status, claimed_until, created_at);

CREATE TABLE IF NOT EXISTS orchestration_evidence (
    id BLOB PRIMARY KEY,
    binding_id BLOB NOT NULL,
    epoch INTEGER NULL,
    kind TEXT NOT NULL,
    subject_revision TEXT NULL,
    payload TEXT NOT NULL,
    source_event_id BLOB NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    FOREIGN KEY(source_event_id) REFERENCES ingress_events(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_orchestration_evidence_binding
    ON orchestration_evidence(binding_id, created_at);

CREATE TABLE IF NOT EXISTS orchestration_adapters (
    id BLOB PRIMARY KEY,
    org_id BLOB NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    current_revision INTEGER NOT NULL,
    enabled INTEGER NOT NULL,
    endpoint_identity TEXT NOT NULL,
    has_admitted_binding INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(org_id, name),
    UNIQUE(endpoint_identity)
);

CREATE TABLE IF NOT EXISTS orchestration_adapter_revisions (
    id BLOB PRIMARY KEY,
    adapter_id BLOB NOT NULL,
    revision INTEGER NOT NULL,
    kind_version TEXT NOT NULL,
    configuration TEXT NOT NULL,
    secret_bindings TEXT NOT NULL,
    identity_configuration TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    actor_id BLOB NULL,
    FOREIGN KEY(adapter_id) REFERENCES orchestration_adapters(id) ON DELETE CASCADE,
    UNIQUE(adapter_id, revision)
);

CREATE TABLE IF NOT EXISTS external_operations (
    id BLOB PRIMARY KEY,
    binding_id BLOB NOT NULL,
    operation_key TEXT NOT NULL,
    provider TEXT NOT NULL,
    action TEXT NOT NULL,
    semantics TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    ambiguous INTEGER NOT NULL DEFAULT 0,
    provenance TEXT NOT NULL,
    receipt TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(binding_id) REFERENCES orchestration_bindings(id) ON DELETE CASCADE,
    UNIQUE(binding_id, operation_key)
);
CREATE INDEX IF NOT EXISTS idx_external_operations_status
    ON external_operations(binding_id, status, updated_at);
