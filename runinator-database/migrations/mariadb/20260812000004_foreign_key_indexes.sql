-- the sibling migration indexes every foreign key column that had nothing leading on it, because an
-- unindexed child column turns a delete of the parent row into a full scan of the child table.
--
-- innodb creates an index for every foreign key column on its own, so every column that migration
-- names is already covered here. workflow_result_events.workflow_run_id is the one exception: it
-- carries no foreign key, so nothing indexes it, and delete_workflow filters on it.
CREATE INDEX idx_workflow_result_events_run ON workflow_result_events(workflow_run_id);
