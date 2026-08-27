ALTER TABLE workspace_leases ADD COLUMN abandonment_notified_at INTEGER NULL;

CREATE INDEX IF NOT EXISTS idx_workspace_abandonment_delivery
    ON workspace_leases(status, abandonment_notified_at, updated_at);

CREATE INDEX IF NOT EXISTS idx_orchestration_bindings_pipeline
    ON orchestration_bindings(pipeline_id);

CREATE INDEX IF NOT EXISTS idx_orchestration_evidence_source_event
    ON orchestration_evidence(source_event_id);
