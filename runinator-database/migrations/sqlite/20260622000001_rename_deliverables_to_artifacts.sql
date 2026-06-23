ALTER TABLE workflow_run_deliverables RENAME TO workflow_run_artifacts;

DROP INDEX IF EXISTS idx_workflow_run_deliverables_run;
CREATE INDEX IF NOT EXISTS idx_workflow_run_artifacts_run ON workflow_run_artifacts(workflow_run_id);
