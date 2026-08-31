-- deleting a parent row makes the engine look for surviving children of every foreign key pointing
-- at it, and with nothing indexing the child column that search is a full scan of the child table --
-- once per deleted parent row. the cost stays invisible until the child table grows, then appears as
-- an operation that never finishes. delete_workflow hit exactly that: it deletes a workflow's node
-- runs, workflow_orchestration_events.workflow_node_run_id had no index, and so each node run
-- scanned the largest table in the schema.
--
-- this indexes every foreign key column that had nothing leading on it. the
-- `every_foreign_key_column_leads_an_index` lint keeps it that way.

-- the delete_workflow path.
CREATE INDEX IF NOT EXISTS idx_workflow_orchestration_events_node_run
    ON workflow_orchestration_events(workflow_node_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_trigger_firings_run
    ON workflow_trigger_firings(workflow_run_id);

-- no foreign key on this one, but delete_workflow filters by it and the table grows per node result.
CREATE INDEX IF NOT EXISTS idx_workflow_result_events_run
    ON workflow_result_events(workflow_run_id);

-- the rest of what deleting a workflow cascades into.
CREATE INDEX IF NOT EXISTS idx_freeze_windows_workflow ON freeze_windows(workflow_id);
CREATE INDEX IF NOT EXISTS idx_notification_policies_workflow ON notification_policies(workflow_id);

-- replicas are pruned on every deployment roll, and each delete scanned both run tables.
CREATE INDEX IF NOT EXISTS idx_workflow_runs_trigger_actor
    ON workflow_runs(trigger_actor_replica_id);
CREATE INDEX IF NOT EXISTS idx_workflow_node_runs_current_executor
    ON workflow_node_runs(current_executor_replica_id);
CREATE INDEX IF NOT EXISTS idx_workflow_node_runs_last_executor
    ON workflow_node_runs(last_executor_replica_id);

-- history the archiver deletes one row at a time, so an unindexed child is a scan per archived row.
CREATE INDEX IF NOT EXISTS idx_notification_deliveries_notification
    ON notification_deliveries(notification_id);
CREATE INDEX IF NOT EXISTS idx_workflow_run_artifacts_artifact
    ON workflow_run_artifacts(artifact_id);

-- and what deleting a pipeline or one of its runs cascades into.
CREATE INDEX IF NOT EXISTS idx_pipeline_runs_pipeline ON pipeline_runs(pipeline_id);
CREATE INDEX IF NOT EXISTS idx_pipeline_triggers_pipeline ON pipeline_triggers(pipeline_id);
CREATE INDEX IF NOT EXISTS idx_pipeline_trigger_firings_run
    ON pipeline_trigger_firings(pipeline_run_id);
