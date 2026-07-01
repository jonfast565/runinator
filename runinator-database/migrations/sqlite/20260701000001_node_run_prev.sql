-- link each node run to the one created immediately before it in the same workflow run, forming a
-- flat guid-linked chain that is easier to debug than the nested `steps` output tree. nullable: the
-- first step in a run has no predecessor.
ALTER TABLE workflow_node_runs ADD COLUMN prev_node_run_id BLOB NULL;
