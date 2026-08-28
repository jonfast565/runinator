-- address a wake and a node run to one thread of control.
--
-- a run holds several cursors once `parallel`/`race` fan out, but a ready-node row named only its
-- `(run, node)`, so nothing could wake one branch without disturbing its siblings and two cursors
-- could never sit on the same node. stamping the cursor makes both possible, which is what the
-- debugger needs to step one branch, and what a speculative "what if" fork needs to walk beside the
-- real one. nullable throughout: rows armed before this migration resolve by node id as before.
ALTER TABLE workflow_ready_nodes ADD COLUMN cursor_id BINARY(16) NULL;
ALTER TABLE workflow_node_runs ADD COLUMN cursor_id BINARY(16) NULL;

-- true when a debugger "what if" cursor produced this node run. persisted independently of the
-- cursor because a retired speculative cursor is gone from run state and this answer must outlive
-- it -- it is what keeps speculative output out of a real branch's `steps` context.
ALTER TABLE workflow_node_runs ADD COLUMN speculative TINYINT(1) NOT NULL DEFAULT 0;

-- the supersede-on-arm lookup, which now narrows to one cursor's own generations.
CREATE INDEX idx_workflow_ready_nodes_run_node_cursor
    ON workflow_ready_nodes(workflow_run_id, node_id, cursor_id);
