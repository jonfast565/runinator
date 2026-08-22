-- delayed re-dispatch for a retried effect. `available_at` is the epoch second before which the
-- publisher must not claim the row; 0 (the default for every pre-existing row) means immediately.
ALTER TABLE workflow_effect_dispatches ADD COLUMN available_at INTEGER NOT NULL DEFAULT 0;
DROP INDEX IF EXISTS idx_workflow_effect_dispatches_pending;
CREATE INDEX IF NOT EXISTS idx_workflow_effect_dispatches_pending
    ON workflow_effect_dispatches(published_at, available_at, created_at, id);
