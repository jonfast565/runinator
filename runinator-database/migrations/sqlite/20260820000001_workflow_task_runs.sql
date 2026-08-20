-- Provider work launched by a RexRap `task[T]` binding. Unlike workflow_node_runs, these rows
-- remain active after the launcher has advanced the parent cursor and own the worker result.
CREATE TABLE IF NOT EXISTS workflow_task_runs (
    id BLOB PRIMARY KEY,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    launch_node_run_id BLOB NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    action TEXT NOT NULL,
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    parameters TEXT NOT NULL DEFAULT '{}',
    output_json TEXT NULL,
    message TEXT NULL,
    current_executor_replica_id BLOB NULL REFERENCES replicas(replica_id),
    last_executor_replica_id BLOB NULL REFERENCES replicas(replica_id),
    executor_claimed_at INTEGER NULL,
    executor_released_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_task_runs_run ON workflow_task_runs(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_task_runs_launch ON workflow_task_runs(launch_node_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_task_runs_status ON workflow_task_runs(status);
