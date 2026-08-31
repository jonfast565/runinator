-- One durable owner for each provider-neutral ingress correlation key.  `org_scope` normalizes
-- the nullable organization id so the unique constraint also protects global definitions.
CREATE TABLE IF NOT EXISTS ingress_admissions (
    id UUID PRIMARY KEY,
    org_scope TEXT NOT NULL,
    scope TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    generation BIGINT NOT NULL,
    workflow_id UUID NULL REFERENCES workflows(id) ON DELETE CASCADE,
    pipeline_id UUID NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    workflow_run_id UUID NULL,
    pipeline_run_id UUID NULL,
    policy TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CHECK ((workflow_id IS NOT NULL) <> (pipeline_id IS NOT NULL)),
    UNIQUE(org_scope, scope, correlation_key)
);
CREATE INDEX IF NOT EXISTS idx_ingress_admissions_workflow ON ingress_admissions(workflow_id);
CREATE INDEX IF NOT EXISTS idx_ingress_admissions_pipeline ON ingress_admissions(pipeline_id);
CREATE TABLE IF NOT EXISTS ingress_events (
    id UUID PRIMARY KEY, admission_id UUID NOT NULL REFERENCES ingress_admissions(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL, generation BIGINT NOT NULL, source TEXT NOT NULL, event_id TEXT NOT NULL,
    event_type TEXT NOT NULL, correlation_key TEXT NOT NULL, payload TEXT NOT NULL,
    occurred_at BIGINT NULL, received_at BIGINT NOT NULL, disposition TEXT NOT NULL,
    queue_state TEXT NOT NULL, claim_token UUID NULL, promoted_generation BIGINT NULL,
    workflow_run_id UUID NULL, pipeline_run_id UUID NULL,
    UNIQUE(admission_id, source, event_id), UNIQUE(admission_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_ingress_events_fifo ON ingress_events(admission_id, queue_state, sequence);
