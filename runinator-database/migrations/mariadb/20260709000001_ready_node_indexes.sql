-- serve the per-run / per-node ready-node cleanup queries — the terminal-run settle in
-- update_workflow_run_status and the poll-supersede inside enqueue_ready_node — which filter by
-- workflow_run_id (and node_id). the existing UNIQUE(source_event_id, workflow_run_id, node_id)
-- leads with source_event_id and cannot seek these, so they scanned the whole table.
CREATE INDEX idx_workflow_ready_nodes_run_node ON workflow_ready_nodes(workflow_run_id, node_id);

-- serve the uncompleted-by-ready_at scan shared by the wake publisher (fetch_pending_ready_nodes)
-- and the terminal-run reaper. the existing claim index leads with status, which neither query
-- constrains. mysql has no partial indexes, so lead with completed_at to keep the IS NULL seek tight.
CREATE INDEX idx_workflow_ready_nodes_pending ON workflow_ready_nodes(completed_at, ready_at, id);
