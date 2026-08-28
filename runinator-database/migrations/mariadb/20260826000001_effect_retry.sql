-- delayed re-dispatch for a retried effect. `available_at` is the epoch second before which the
-- publisher must not claim the row; 0 (the default for every pre-existing row) means immediately.
ALTER TABLE workflow_effect_dispatches ADD COLUMN available_at BIGINT NOT NULL DEFAULT 0;
DROP INDEX idx_workflow_effect_dispatches_pending ON workflow_effect_dispatches;
CREATE INDEX idx_workflow_effect_dispatches_pending
    ON workflow_effect_dispatches(published_at, available_at, created_at, id);
