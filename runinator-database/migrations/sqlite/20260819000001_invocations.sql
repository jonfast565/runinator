-- durable state for the resumable invocation vm.
--
-- an invocation is one authored node's program, frozen between the durable calls it makes. the
-- continuation column is the whole point: it holds the frame stack, each frame's instruction
-- pointer, operand stack and locals, so a program suspended on a call in one replica resumes in
-- another. the module itself is *not* stored here — it lives in the run's workflow snapshot, which
-- is already the thing insulating an in-flight run from a redefinition — only the version it was
-- frozen against, so a resume can refuse a continuation the current module would misread.
CREATE TABLE IF NOT EXISTS workflow_invocations (
    id BLOB PRIMARY KEY,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    workflow_node_run_id BLOB NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    -- which thread of control owns this. a fan-out can have one invocation per branch on the same
    -- node, so the cursor is part of the identity, exactly as it is for ready nodes and node runs.
    cursor_id BLOB NULL,
    node_id TEXT NOT NULL,
    module_version INTEGER NOT NULL,
    continuation TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL,
    output_json TEXT NULL,
    message TEXT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    finished_at INTEGER NULL
);

CREATE INDEX IF NOT EXISTS idx_workflow_invocations_run ON workflow_invocations(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_invocations_node_run ON workflow_invocations(workflow_node_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_invocations_cursor ON workflow_invocations(cursor_id);
CREATE INDEX IF NOT EXISTS idx_workflow_invocations_status ON workflow_invocations(status);

-- one durable call an invocation yielded on.
--
-- this is the row that breaks the old one-node-run-to-one-dispatch assumption: an invocation makes N
-- of these under a single node run, so the per-attempt state a node run used to hold alone — the
-- attempt counter, the executor lease, the deadline — lives here instead. `sequence` is assigned by
-- the vm's own call counter, which makes it stable across a replay: re-running a resumed program
-- reaches the same call with the same sequence, so the unique index below turns a duplicated drive
-- into a no-op rather than a second dispatch.
CREATE TABLE IF NOT EXISTS workflow_invocation_calls (
    id BLOB PRIMARY KEY,
    invocation_id BLOB NOT NULL REFERENCES workflow_invocations(id) ON DELETE CASCADE,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    target TEXT NOT NULL,
    arguments TEXT NOT NULL DEFAULT '[]',
    policy TEXT NOT NULL DEFAULT '{}',
    attempt INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    result_json TEXT NULL,
    message TEXT NULL,
    idempotency_key TEXT NULL,
    deadline_at INTEGER NULL,
    current_executor_replica_id BLOB NULL REFERENCES replicas(replica_id),
    last_executor_replica_id BLOB NULL REFERENCES replicas(replica_id),
    executor_claimed_at INTEGER NULL,
    executor_released_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL
);

-- the call's identity within its invocation. this is what makes a resumed program idempotent.
CREATE UNIQUE INDEX IF NOT EXISTS idx_workflow_invocation_calls_sequence
    ON workflow_invocation_calls(invocation_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflow_invocation_calls_run ON workflow_invocation_calls(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_invocation_calls_status ON workflow_invocation_calls(status);
CREATE INDEX IF NOT EXISTS idx_workflow_invocation_calls_current_executor
    ON workflow_invocation_calls(current_executor_replica_id);
CREATE INDEX IF NOT EXISTS idx_workflow_invocation_calls_last_executor
    ON workflow_invocation_calls(last_executor_replica_id);

-- chunks and artifacts are attributed to the node run as before, plus the call that produced them
-- when one did. nullable because every pre-invocation row has no call, and because a node kind that
-- is not an invocation still produces both.
ALTER TABLE workflow_node_chunks ADD COLUMN invocation_call_id BLOB NULL REFERENCES workflow_invocation_calls(id) ON DELETE CASCADE;
ALTER TABLE workflow_node_artifacts ADD COLUMN invocation_call_id BLOB NULL REFERENCES workflow_invocation_calls(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_workflow_node_chunks_invocation_call ON workflow_node_chunks(invocation_call_id);
CREATE INDEX IF NOT EXISTS idx_workflow_node_artifacts_invocation_call ON workflow_node_artifacts(invocation_call_id);
