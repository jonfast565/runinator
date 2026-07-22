-- pipeline-level triggers (cron/manual/chained). owned by a pipeline; a chained trigger is
-- target-keyed (pipeline_id is the pipeline to start) with its source and `on` selector in
-- configuration. mirrors workflow_triggers.
CREATE TABLE IF NOT EXISTS pipeline_triggers (
    id UUID PRIMARY KEY,
    pipeline_id UUID NOT NULL REFERENCES pipelines(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    configuration TEXT NOT NULL DEFAULT '{}',
    next_execution BIGINT NULL,
    blackout_start BIGINT NULL,
    blackout_end BIGINT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- first-class pipeline runs: an orchestration envelope over member workflow runs.
CREATE TABLE IF NOT EXISTS pipeline_runs (
    id UUID PRIMARY KEY,
    pipeline_id UUID NOT NULL REFERENCES pipelines(id),
    pipeline_snapshot TEXT NULL,
    status TEXT NOT NULL,
    parameters TEXT NOT NULL DEFAULT '{}',
    state TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    message TEXT NULL,
    trigger_source_kind TEXT NULL,
    trigger_actor_type TEXT NULL,
    trigger_actor_replica_id UUID NULL,
    trigger_actor_display_name TEXT NULL,
    trigger_metadata TEXT NOT NULL DEFAULT '{}'
);

-- exactly-once ledger for pipeline trigger firings (cron next_execution / chained source-run id).
CREATE TABLE IF NOT EXISTS pipeline_trigger_firings (
    id UUID PRIMARY KEY,
    trigger_id UUID NOT NULL REFERENCES pipeline_triggers(id) ON DELETE CASCADE,
    fire_key TEXT NOT NULL,
    pipeline_run_id UUID NULL REFERENCES pipeline_runs(id),
    scheduler_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    UNIQUE(trigger_id, fire_key)
);

-- tag a member workflow run with the owning pipeline run so the orchestrator can aggregate.
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS pipeline_run_id UUID NULL REFERENCES pipeline_runs(id);

CREATE INDEX IF NOT EXISTS idx_pipeline_runs_status ON pipeline_runs(status);
CREATE INDEX IF NOT EXISTS idx_pipeline_triggers_due ON pipeline_triggers(enabled, kind, next_execution);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_pipeline_run ON workflow_runs(pipeline_run_id);
