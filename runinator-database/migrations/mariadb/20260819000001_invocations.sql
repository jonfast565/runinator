-- durable state for the resumable invocation vm.
--
-- mysql notes, all of which differ from the other two dialects: foreign keys are declared
-- table-level because mysql 8 silently discards a column-level `REFERENCES`; `CREATE INDEX` takes no
-- `IF NOT EXISTS`; `ALTER TABLE ... ADD COLUMN` takes no `IF NOT EXISTS`; and a TEXT column cannot
-- carry a literal DEFAULT, so the json columns are supplied by every writer instead of defaulted.
--
-- an invocation is one authored node's program, frozen between the durable calls it makes. the
-- continuation column is the whole point: it holds the frame stack, each frame's instruction
-- pointer, operand stack and locals, so a program suspended on a call in one replica resumes in
-- another. the module itself is *not* stored here — it lives in the run's workflow snapshot, which
-- is already the thing insulating an in-flight run from a redefinition — only the version it was
-- frozen against, so a resume can refuse a continuation the current module would misread.
CREATE TABLE IF NOT EXISTS workflow_invocations (
    id BINARY(16) PRIMARY KEY,
    workflow_run_id BINARY(16) NOT NULL,
    workflow_node_run_id BINARY(16) NOT NULL,
    -- which thread of control owns this. a fan-out can have one invocation per branch on the same
    -- node, so the cursor is part of the identity, exactly as it is for ready nodes and node runs.
    cursor_id BINARY(16) NULL,
    node_id VARCHAR(255) NOT NULL,
    module_version BIGINT NOT NULL,
    continuation LONGTEXT NOT NULL,
    status VARCHAR(64) NOT NULL,
    output_json LONGTEXT NULL,
    message TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    finished_at BIGINT NULL,
    CONSTRAINT fk_workflow_invocations_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_invocations_node_run FOREIGN KEY (workflow_node_run_id)
        REFERENCES workflow_node_runs(id) ON DELETE CASCADE
);

CREATE INDEX idx_workflow_invocations_run ON workflow_invocations(workflow_run_id);
CREATE INDEX idx_workflow_invocations_node_run ON workflow_invocations(workflow_node_run_id);
CREATE INDEX idx_workflow_invocations_cursor ON workflow_invocations(cursor_id);
CREATE INDEX idx_workflow_invocations_status ON workflow_invocations(status);

-- one durable call an invocation yielded on.
--
-- this is the row that breaks the old one-node-run-to-one-dispatch assumption: an invocation makes N
-- of these under a single node run, so the per-attempt state a node run used to hold alone — the
-- attempt counter, the executor lease, the deadline — lives here instead. `sequence` is assigned by
-- the vm's own call counter, which makes it stable across a replay: re-running a resumed program
-- reaches the same call with the same sequence, so the unique index below turns a duplicated drive
-- into a no-op rather than a second dispatch.
CREATE TABLE IF NOT EXISTS workflow_invocation_calls (
    id BINARY(16) PRIMARY KEY,
    invocation_id BINARY(16) NOT NULL,
    workflow_run_id BINARY(16) NOT NULL,
    sequence BIGINT NOT NULL,
    target LONGTEXT NOT NULL,
    arguments LONGTEXT NOT NULL,
    policy LONGTEXT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0,
    status VARCHAR(64) NOT NULL,
    result_json LONGTEXT NULL,
    message TEXT NULL,
    idempotency_key VARCHAR(512) NULL,
    deadline_at BIGINT NULL,
    current_executor_replica_id BINARY(16) NULL,
    last_executor_replica_id BINARY(16) NULL,
    executor_claimed_at BIGINT NULL,
    executor_released_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    CONSTRAINT fk_workflow_invocation_calls_invocation FOREIGN KEY (invocation_id)
        REFERENCES workflow_invocations(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_invocation_calls_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_invocation_calls_current_executor FOREIGN KEY (current_executor_replica_id)
        REFERENCES replicas(replica_id),
    CONSTRAINT fk_workflow_invocation_calls_last_executor FOREIGN KEY (last_executor_replica_id)
        REFERENCES replicas(replica_id)
);

-- the call's identity within its invocation. this is what makes a resumed program idempotent.
CREATE UNIQUE INDEX idx_workflow_invocation_calls_sequence
    ON workflow_invocation_calls(invocation_id, sequence);
CREATE INDEX idx_workflow_invocation_calls_run ON workflow_invocation_calls(workflow_run_id);
CREATE INDEX idx_workflow_invocation_calls_status ON workflow_invocation_calls(status);
CREATE INDEX idx_workflow_invocation_calls_current_executor
    ON workflow_invocation_calls(current_executor_replica_id);
CREATE INDEX idx_workflow_invocation_calls_last_executor
    ON workflow_invocation_calls(last_executor_replica_id);

-- chunks and artifacts are attributed to the node run as before, plus the call that produced them
-- when one did. nullable because every pre-invocation row has no call, and because a node kind that
-- is not an invocation still produces both.
ALTER TABLE workflow_node_chunks ADD COLUMN invocation_call_id BINARY(16) NULL;
ALTER TABLE workflow_node_artifacts ADD COLUMN invocation_call_id BINARY(16) NULL;

CREATE INDEX idx_workflow_node_chunks_invocation_call ON workflow_node_chunks(invocation_call_id);
CREATE INDEX idx_workflow_node_artifacts_invocation_call ON workflow_node_artifacts(invocation_call_id);

ALTER TABLE workflow_node_chunks ADD CONSTRAINT fk_workflow_node_chunks_invocation_call
    FOREIGN KEY (invocation_call_id) REFERENCES workflow_invocation_calls(id) ON DELETE CASCADE;
ALTER TABLE workflow_node_artifacts ADD CONSTRAINT fk_workflow_node_artifacts_invocation_call
    FOREIGN KEY (invocation_call_id) REFERENCES workflow_invocation_calls(id) ON DELETE CASCADE;
