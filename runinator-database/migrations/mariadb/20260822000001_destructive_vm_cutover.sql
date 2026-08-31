-- Destructive VM cutover. Apply only after every legacy engine, waker, and worker has stopped
-- and all nonterminal legacy runs have been cancelled. Continuations, effects, and journal
-- entries are now the sole workflow execution history.
DROP TABLE IF EXISTS workflow_task_runs;
DROP TABLE IF EXISTS workflow_mutex_waiters;
DROP TABLE IF EXISTS workflow_run_artifacts;
DROP TABLE IF EXISTS workflow_node_chunks;
DROP TABLE IF EXISTS workflow_node_artifacts;
DROP TABLE IF EXISTS workflow_invocation_calls;
DROP TABLE IF EXISTS workflow_invocations;
DROP TABLE IF EXISTS workflow_result_events;
DROP TABLE IF EXISTS workflow_ready_nodes;
DROP TABLE IF EXISTS workflow_orchestration_events;
DROP TABLE IF EXISTS workflow_action_dispatches;
DROP TABLE IF EXISTS workflow_node_runs;

-- The mutex itself remains a VM coordination primitive; only its legacy cursor spelling changes.
ALTER TABLE workflow_mutexes RENAME COLUMN holder_cursor_id TO holder_continuation_id;
