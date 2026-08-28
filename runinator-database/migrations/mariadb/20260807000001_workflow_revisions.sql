-- append-only history for authored workflow definitions. the `workflows` row remains the mutable
-- head; every accepted definition is also captured here so a change can be diffed, attributed, and
-- rolled back. in-flight runs are already insulated by `workflow_runs.workflow_snapshot`, so this
-- exists to answer "what changed, who changed it, and how do i get back" — not to make runs safe.
CREATE TABLE IF NOT EXISTS workflow_revisions (
    id BINARY(16) PRIMARY KEY,
    workflow_id BINARY(16) NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL,
    version VARCHAR(64) NOT NULL,
    name VARCHAR(255) NOT NULL,
    definition LONGTEXT NOT NULL,
    input_schema LONGTEXT NOT NULL,
    source VARCHAR(32) NOT NULL DEFAULT 'api',
    actor_id BINARY(16) NULL,
    actor_kind VARCHAR(64) NOT NULL DEFAULT 'unknown',
    note TEXT NULL,
    created_at BIGINT NOT NULL
);

-- the sequence is the rollback handle, so make a duplicate number impossible rather than merely
-- unlikely: two replicas racing the same save lose one insert instead of forking history.
CREATE UNIQUE INDEX idx_workflow_revisions_seq
    ON workflow_revisions(workflow_id, revision);

-- listing a workflow's history is newest-first over one workflow.
CREATE INDEX idx_workflow_revisions_created
    ON workflow_revisions(workflow_id, created_at);
