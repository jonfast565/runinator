-- normalized, cursor-scoped workflow mutex ownership and durable fifo waiters.
CREATE TABLE IF NOT EXISTS workflow_mutexes (
    name TEXT PRIMARY KEY,
    holder_run_id UUID NULL REFERENCES workflow_runs(id) ON DELETE SET NULL,
    holder_cursor_id UUID NULL,
    acquired_at BIGINT NULL,
    hold_deadline BIGINT NULL,
    overdue_at BIGINT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_mutex_waiters (
    workflow_node_run_id UUID PRIMARY KEY REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    workflow_run_id UUID NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    cursor_id UUID NOT NULL,
    node_id TEXT NOT NULL,
    enqueued_at BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_workflow_mutex_waiters_fifo
    ON workflow_mutex_waiters(name, enqueued_at, workflow_node_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_mutex_waiters_run
    ON workflow_mutex_waiters(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_mutexes_holder
    ON workflow_mutexes(holder_run_id);
