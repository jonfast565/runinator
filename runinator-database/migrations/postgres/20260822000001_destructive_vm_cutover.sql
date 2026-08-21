-- Destructive VM cutover. Apply only after every legacy engine, waker, and worker has stopped
-- and all nonterminal legacy runs have been cancelled. Continuations, effects, and journal
-- entries are now the sole workflow execution history. The ready-node queue remains the
-- scheduler's durable work queue during the VM transition.
DROP TABLE IF EXISTS workflow_task_runs;
-- These tables reference workflow_invocation_calls, so they must be removed before
-- the invocation history they belong to.
DROP TABLE IF EXISTS workflow_run_artifacts;
DROP TABLE IF EXISTS workflow_node_chunks;
DROP TABLE IF EXISTS workflow_node_artifacts;
DROP TABLE IF EXISTS workflow_invocation_calls;
DROP TABLE IF EXISTS workflow_invocations;
DROP TABLE IF EXISTS workflow_mutex_waiters;
DROP TABLE IF EXISTS workflow_result_events;
DROP TABLE IF EXISTS workflow_orchestration_events;
DROP TABLE IF EXISTS workflow_action_dispatches;
DROP TABLE IF EXISTS workflow_node_runs;

-- The mutex itself remains a VM coordination primitive; only its legacy cursor spelling changes.
ALTER TABLE workflow_mutexes RENAME COLUMN holder_cursor_id TO holder_continuation_id;

CREATE INDEX IF NOT EXISTS idx_workflow_effect_output_run
    ON workflow_effect_output_events(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_effect_output_continuation
    ON workflow_effect_output_events(continuation_id);
