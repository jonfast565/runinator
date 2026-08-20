-- Canonical durable state for the compiled workflow VM. These tables deliberately do not refer to
-- node runs: continuations and effects are the execution identity.
CREATE TABLE IF NOT EXISTS workflow_vm_modules (
    workflow_run_id UUID PRIMARY KEY REFERENCES workflow_runs(id) ON DELETE CASCADE,
    version BIGINT NOT NULL,
    module_json TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_continuations (
    id UUID PRIMARY KEY,
    workflow_run_id UUID NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    module_version BIGINT NOT NULL,
    continuation_json TEXT NOT NULL,
    status TEXT NOT NULL,
    version BIGINT NOT NULL DEFAULT 0,
    ready_at BIGINT NULL,
    claimed_by TEXT NULL,
    claimed_until BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_continuations_runnable
    ON workflow_continuations(status, ready_at, id);
CREATE INDEX IF NOT EXISTS idx_workflow_continuations_run
    ON workflow_continuations(workflow_run_id, created_at);

CREATE TABLE IF NOT EXISTS workflow_effects (
    id UUID PRIMARY KEY,
    version BIGINT NOT NULL,
    workflow_run_id UUID NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    continuation_id UUID NOT NULL REFERENCES workflow_continuations(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0,
    request_json TEXT NOT NULL,
    status TEXT NOT NULL,
    result_json TEXT NULL,
    message TEXT NULL,
    idempotency_key TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    finished_at BIGINT NULL,
    UNIQUE(continuation_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_workflow_effects_pending
    ON workflow_effects(status, created_at, id);
CREATE INDEX IF NOT EXISTS idx_workflow_effects_run
    ON workflow_effects(workflow_run_id, created_at);

CREATE TABLE IF NOT EXISTS workflow_effect_dispatches (
    id UUID PRIMARY KEY,
    effect_id UUID NOT NULL REFERENCES workflow_effects(id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL UNIQUE,
    command_json TEXT NOT NULL,
    attempts BIGINT NOT NULL DEFAULT 0,
    published_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    last_error TEXT NULL,
    claimed_by TEXT NULL,
    claimed_until BIGINT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_effect_dispatches_pending
    ON workflow_effect_dispatches(published_at, created_at, id);
CREATE INDEX IF NOT EXISTS idx_workflow_effect_dispatches_effect
    ON workflow_effect_dispatches(effect_id);

CREATE TABLE IF NOT EXISTS workflow_journal_entries (
    id UUID PRIMARY KEY,
    version BIGINT NOT NULL,
    workflow_run_id UUID NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL,
    continuation_id UUID NULL REFERENCES workflow_continuations(id) ON DELETE SET NULL,
    effect_id UUID NULL REFERENCES workflow_effects(id) ON DELETE SET NULL,
    entry_json TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE(workflow_run_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_workflow_journal_entries_run
    ON workflow_journal_entries(workflow_run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflow_journal_entries_continuation
    ON workflow_journal_entries(continuation_id);
CREATE INDEX IF NOT EXISTS idx_workflow_journal_entries_effect
    ON workflow_journal_entries(effect_id);
