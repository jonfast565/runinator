-- append-only history for authored workflow definitions. the `workflows` row remains the mutable
-- head; every accepted definition is also captured here so a change can be diffed, attributed, and
-- rolled back. in-flight runs are already insulated by `workflow_runs.workflow_snapshot`, so this
-- exists to answer "what changed, who changed it, and how do i get back" — not to make runs safe.
CREATE TABLE IF NOT EXISTS workflow_revisions (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL,
    version TEXT NOT NULL,
    name TEXT NOT NULL,
    definition TEXT NOT NULL,
    input_schema TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'api',
    actor_id UUID NULL,
    actor_kind TEXT NOT NULL DEFAULT 'unknown',
    note TEXT NULL,
    created_at BIGINT NOT NULL
);

-- the sequence is the rollback handle, so make a duplicate number impossible rather than merely
-- unlikely: two replicas racing the same save lose one insert instead of forking history.
CREATE UNIQUE INDEX IF NOT EXISTS idx_workflow_revisions_seq
    ON workflow_revisions(workflow_id, revision);

-- listing a workflow's history is newest-first over one workflow.
CREATE INDEX IF NOT EXISTS idx_workflow_revisions_created
    ON workflow_revisions(workflow_id, created_at);
