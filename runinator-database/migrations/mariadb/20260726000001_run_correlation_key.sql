-- optional stable identity for a run, matched by `await workflow ... key` joins. set at start
-- (trigger/api/subflow) or stamped from the workflow's `metadata.correlation` expression as the run
-- progresses. nullable: most runs carry no correlation key.
ALTER TABLE workflow_runs ADD COLUMN correlation_key VARCHAR(255) NULL;
CREATE INDEX idx_workflow_runs_correlation ON workflow_runs(workflow_id, correlation_key);
