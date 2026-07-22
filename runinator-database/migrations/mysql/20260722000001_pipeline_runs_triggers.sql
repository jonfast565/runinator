-- pipeline-level triggers (cron/manual/chained). owned by a pipeline; a chained trigger is
-- target-keyed (pipeline_id is the pipeline to start) with its source and `on` selector in
-- configuration. mirrors workflow_triggers.
CREATE TABLE IF NOT EXISTS pipeline_triggers (
    id BINARY(16) PRIMARY KEY,
    pipeline_id BINARY(16) NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    kind VARCHAR(64) NOT NULL,
    enabled TINYINT(1) NOT NULL,
    configuration LONGTEXT NOT NULL,
    next_execution BIGINT NULL,
    blackout_start BIGINT NULL,
    blackout_end BIGINT NULL,
    metadata LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- first-class pipeline runs: an orchestration envelope over member workflow runs.
CREATE TABLE IF NOT EXISTS pipeline_runs (
    id BINARY(16) PRIMARY KEY,
    pipeline_id BINARY(16) NOT NULL REFERENCES pipelines(id),
    pipeline_snapshot LONGTEXT NULL,
    status VARCHAR(64) NOT NULL,
    parameters LONGTEXT NOT NULL,
    state LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    message TEXT NULL,
    trigger_source_kind TEXT NULL,
    trigger_actor_type TEXT NULL,
    trigger_actor_replica_id BINARY(16) NULL,
    trigger_actor_display_name TEXT NULL,
    trigger_metadata TEXT NOT NULL DEFAULT '{}'
);

-- exactly-once ledger for pipeline trigger firings (cron next_execution / chained source-run id).
CREATE TABLE IF NOT EXISTS pipeline_trigger_firings (
    id BINARY(16) PRIMARY KEY,
    trigger_id BINARY(16) NOT NULL REFERENCES pipeline_triggers(id) ON DELETE CASCADE,
    fire_key VARCHAR(255) NOT NULL,
    pipeline_run_id BINARY(16) NULL REFERENCES pipeline_runs(id),
    scheduler_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE(trigger_id, fire_key)
);

-- tag a member workflow run with the owning pipeline run so the orchestrator can aggregate.
ALTER TABLE workflow_runs ADD COLUMN pipeline_run_id BINARY(16) NULL;

CREATE INDEX idx_pipeline_runs_status ON pipeline_runs(status);
CREATE INDEX idx_pipeline_triggers_due ON pipeline_triggers(enabled, kind, next_execution);
CREATE INDEX idx_workflow_runs_pipeline_run ON workflow_runs(pipeline_run_id);
