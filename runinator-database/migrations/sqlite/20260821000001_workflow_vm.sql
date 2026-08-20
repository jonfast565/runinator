-- Canonical durable state for the compiled workflow VM. These tables deliberately do not refer to
-- node runs: continuations and effects are the execution identity.
CREATE TABLE IF NOT EXISTS workflow_vm_modules (
    workflow_run_id BLOB PRIMARY KEY REFERENCES workflow_runs(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    module_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_continuations (
    id BLOB PRIMARY KEY,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    module_version INTEGER NOT NULL,
    continuation_json TEXT NOT NULL,
    status TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0,
    ready_at INTEGER NULL,
    claimed_by TEXT NULL,
    claimed_until INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_continuations_runnable
    ON workflow_continuations(status, ready_at, id);
CREATE INDEX IF NOT EXISTS idx_workflow_continuations_run
    ON workflow_continuations(workflow_run_id, created_at);

CREATE TABLE IF NOT EXISTS workflow_effects (
    id BLOB PRIMARY KEY,
    version INTEGER NOT NULL,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    continuation_id BLOB NOT NULL REFERENCES workflow_continuations(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    request_json TEXT NOT NULL,
    status TEXT NOT NULL,
    result_json TEXT NULL,
    message TEXT NULL,
    idempotency_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    finished_at INTEGER NULL,
    UNIQUE(continuation_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_workflow_effects_pending
    ON workflow_effects(status, created_at, id);
CREATE INDEX IF NOT EXISTS idx_workflow_effects_run
    ON workflow_effects(workflow_run_id, created_at);

CREATE TABLE IF NOT EXISTS workflow_effect_dispatches (
    id BLOB PRIMARY KEY,
    effect_id BLOB NOT NULL REFERENCES workflow_effects(id) ON DELETE CASCADE,
    dedupe_key TEXT NOT NULL UNIQUE,
    command_json TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    published_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_error TEXT NULL,
    claimed_by TEXT NULL,
    claimed_until INTEGER NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_effect_dispatches_pending
    ON workflow_effect_dispatches(published_at, created_at, id);
CREATE INDEX IF NOT EXISTS idx_workflow_effect_dispatches_effect
    ON workflow_effect_dispatches(effect_id);

CREATE TABLE IF NOT EXISTS workflow_journal_entries (
    id BLOB PRIMARY KEY,
    version INTEGER NOT NULL,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    continuation_id BLOB NULL REFERENCES workflow_continuations(id) ON DELETE SET NULL,
    effect_id BLOB NULL REFERENCES workflow_effects(id) ON DELETE SET NULL,
    entry_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(workflow_run_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_workflow_journal_entries_run
    ON workflow_journal_entries(workflow_run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflow_journal_entries_continuation
    ON workflow_journal_entries(continuation_id);
CREATE INDEX IF NOT EXISTS idx_workflow_journal_entries_effect
    ON workflow_journal_entries(effect_id);
