-- Canonical durable state for the compiled workflow VM. MySQL uses table-level foreign keys and
-- omits `IF NOT EXISTS` from index creation; this migration is applied once per schema version.
CREATE TABLE IF NOT EXISTS workflow_vm_modules (
    workflow_run_id BINARY(16) PRIMARY KEY,
    version BIGINT NOT NULL,
    module_json LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    CONSTRAINT fk_workflow_vm_modules_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS workflow_continuations (
    id BINARY(16) PRIMARY KEY,
    workflow_run_id BINARY(16) NOT NULL,
    module_version BIGINT NOT NULL,
    continuation_json LONGTEXT NOT NULL,
    status VARCHAR(64) NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    ready_at BIGINT NULL,
    claimed_by TEXT NULL,
    claimed_until BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_workflow_continuations_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE
);
CREATE INDEX idx_workflow_continuations_runnable
    ON workflow_continuations(status, ready_at, id);
CREATE INDEX idx_workflow_continuations_run
    ON workflow_continuations(workflow_run_id, created_at);

CREATE TABLE IF NOT EXISTS workflow_effects (
    id BINARY(16) PRIMARY KEY,
    version BIGINT NOT NULL,
    workflow_run_id BINARY(16) NOT NULL,
    continuation_id BINARY(16) NOT NULL,
    sequence BIGINT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0,
    request_json LONGTEXT NOT NULL,
    status VARCHAR(64) NOT NULL,
    result_json LONGTEXT NULL,
    message TEXT NULL,
    idempotency_key VARCHAR(512) NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    finished_at BIGINT NULL,
    UNIQUE KEY idx_workflow_effects_sequence (continuation_id, sequence),
    CONSTRAINT fk_workflow_effects_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_effects_continuation FOREIGN KEY (continuation_id)
        REFERENCES workflow_continuations(id) ON DELETE CASCADE
);
CREATE INDEX idx_workflow_effects_pending
    ON workflow_effects(status, created_at, id);
CREATE INDEX idx_workflow_effects_run
    ON workflow_effects(workflow_run_id, created_at);

CREATE TABLE IF NOT EXISTS workflow_effect_output_events (
    event_id BINARY(16) PRIMARY KEY,
    effect_id BINARY(16) NOT NULL,
    workflow_run_id BINARY(16) NOT NULL,
    continuation_id BINARY(16) NOT NULL,
    attempt BIGINT NOT NULL,
    output_json TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    CONSTRAINT fk_workflow_effect_output_effect FOREIGN KEY (effect_id)
        REFERENCES workflow_effects(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_effect_output_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_effect_output_continuation FOREIGN KEY (continuation_id)
        REFERENCES workflow_continuations(id) ON DELETE CASCADE
);
CREATE INDEX idx_workflow_effect_output_effect
    ON workflow_effect_output_events(effect_id, created_at, event_id);

CREATE TABLE IF NOT EXISTS workflow_effect_dispatches (
    id BINARY(16) PRIMARY KEY,
    effect_id BINARY(16) NOT NULL,
    dedupe_key VARCHAR(512) NOT NULL UNIQUE,
    command_json LONGTEXT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    published_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    last_error TEXT NULL,
    claimed_by TEXT NULL,
    claimed_until BIGINT NULL,
    CONSTRAINT fk_workflow_effect_dispatches_effect FOREIGN KEY (effect_id)
        REFERENCES workflow_effects(id) ON DELETE CASCADE
);
CREATE INDEX idx_workflow_effect_dispatches_pending
    ON workflow_effect_dispatches(published_at, created_at, id);
CREATE INDEX idx_workflow_effect_dispatches_effect
    ON workflow_effect_dispatches(effect_id);

CREATE TABLE IF NOT EXISTS workflow_journal_entries (
    id BINARY(16) PRIMARY KEY,
    version BIGINT NOT NULL,
    workflow_run_id BINARY(16) NOT NULL,
    sequence BIGINT NOT NULL,
    continuation_id BINARY(16) NULL,
    effect_id BINARY(16) NULL,
    entry_json LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE KEY idx_workflow_journal_entries_sequence (workflow_run_id, sequence),
    CONSTRAINT fk_workflow_journal_entries_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_journal_entries_continuation FOREIGN KEY (continuation_id)
        REFERENCES workflow_continuations(id) ON DELETE SET NULL,
    CONSTRAINT fk_workflow_journal_entries_effect FOREIGN KEY (effect_id)
        REFERENCES workflow_effects(id) ON DELETE SET NULL
);
CREATE INDEX idx_workflow_journal_entries_run
    ON workflow_journal_entries(workflow_run_id, sequence);
CREATE INDEX idx_workflow_journal_entries_continuation
    ON workflow_journal_entries(continuation_id);
CREATE INDEX idx_workflow_journal_entries_effect
    ON workflow_journal_entries(effect_id);
