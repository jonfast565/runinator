-- The VM's executor lease. Replica load and dead-worker recovery used to read
-- workflow_node_runs; that table is gone, so the claim lives on the effect the host is
-- actually executing.
ALTER TABLE workflow_effects ADD COLUMN current_executor_replica_id BINARY(16) NULL;
ALTER TABLE workflow_effects ADD COLUMN last_executor_replica_id BINARY(16) NULL;
CREATE INDEX idx_workflow_effects_current_executor
    ON workflow_effects(current_executor_replica_id);
