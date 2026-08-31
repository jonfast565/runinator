-- One durable owner for each provider-neutral ingress correlation key.  `org_scope` normalizes
-- the nullable organization id so the unique constraint also protects global definitions.
CREATE TABLE IF NOT EXISTS ingress_admissions (
    id BLOB PRIMARY KEY,
    org_scope TEXT NOT NULL,
    scope TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    generation INTEGER NOT NULL,
    workflow_id BLOB NULL,
    pipeline_id BLOB NULL,
    status TEXT NOT NULL,
    workflow_run_id BLOB NULL,
    pipeline_run_id BLOB NULL,
    policy TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(workflow_id) REFERENCES workflows(id) ON DELETE CASCADE,
    FOREIGN KEY(pipeline_id) REFERENCES pipelines(id) ON DELETE CASCADE,
    CHECK ((workflow_id IS NOT NULL) <> (pipeline_id IS NOT NULL)),
    UNIQUE(org_scope, scope, correlation_key)
);
CREATE INDEX IF NOT EXISTS idx_ingress_admissions_workflow ON ingress_admissions(workflow_id);
CREATE INDEX IF NOT EXISTS idx_ingress_admissions_pipeline ON ingress_admissions(pipeline_id);

CREATE TABLE IF NOT EXISTS ingress_events (
    id BLOB PRIMARY KEY,
    admission_id BLOB NOT NULL,
    sequence INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    source TEXT NOT NULL,
    event_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    correlation_key TEXT NOT NULL,
    payload TEXT NOT NULL,
    occurred_at INTEGER NULL,
    received_at INTEGER NOT NULL,
    disposition TEXT NOT NULL,
    queue_state TEXT NOT NULL,
    claim_token BLOB NULL,
    promoted_generation INTEGER NULL,
    workflow_run_id BLOB NULL,
    pipeline_run_id BLOB NULL,
    FOREIGN KEY(admission_id) REFERENCES ingress_admissions(id) ON DELETE CASCADE,
    UNIQUE(admission_id, source, event_id),
    UNIQUE(admission_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_ingress_events_fifo ON ingress_events(admission_id, queue_state, sequence);
