CREATE TABLE IF NOT EXISTS workflow_task_runs (
    id BINARY(16) PRIMARY KEY,
    workflow_run_id BINARY(16) NOT NULL,
    launch_node_run_id BINARY(16) NOT NULL,
    node_id TEXT NOT NULL,
    action LONGTEXT NOT NULL,
    status VARCHAR(64) NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0,
    parameters LONGTEXT NOT NULL,
    output_json LONGTEXT NULL,
    message TEXT NULL,
    current_executor_replica_id BINARY(16) NULL,
    last_executor_replica_id BINARY(16) NULL,
    executor_claimed_at BIGINT NULL,
    executor_released_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    CONSTRAINT fk_workflow_task_runs_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_task_runs_launch FOREIGN KEY (launch_node_run_id)
        REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_task_runs_current_executor FOREIGN KEY (current_executor_replica_id)
        REFERENCES replicas(replica_id),
    CONSTRAINT fk_workflow_task_runs_last_executor FOREIGN KEY (last_executor_replica_id)
        REFERENCES replicas(replica_id)
);
CREATE INDEX idx_workflow_task_runs_run ON workflow_task_runs(workflow_run_id);
CREATE INDEX idx_workflow_task_runs_launch ON workflow_task_runs(launch_node_run_id);
CREATE INDEX idx_workflow_task_runs_status ON workflow_task_runs(status);
