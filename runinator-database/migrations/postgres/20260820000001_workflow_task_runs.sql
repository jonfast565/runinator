CREATE TABLE IF NOT EXISTS workflow_task_runs (
    id UUID PRIMARY KEY,
    workflow_run_id UUID NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    launch_node_run_id UUID NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    action TEXT NOT NULL,
    status TEXT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0,
    parameters TEXT NOT NULL DEFAULT '{}',
    output_json TEXT NULL,
    message TEXT NULL,
    current_executor_replica_id UUID NULL REFERENCES replicas(replica_id),
    last_executor_replica_id UUID NULL REFERENCES replicas(replica_id),
    executor_claimed_at BIGINT NULL,
    executor_released_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_task_runs_run ON workflow_task_runs(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_task_runs_launch ON workflow_task_runs(launch_node_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_task_runs_status ON workflow_task_runs(status);
