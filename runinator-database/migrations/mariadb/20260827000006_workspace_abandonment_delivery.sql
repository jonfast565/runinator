ALTER TABLE workspace_leases ADD COLUMN abandonment_notified_at BIGINT NULL;

CREATE INDEX idx_workspace_abandonment_delivery
    ON workspace_leases(status, abandonment_notified_at, updated_at);

CREATE INDEX idx_orchestration_evidence_source_event
    ON orchestration_evidence(source_event_id);
