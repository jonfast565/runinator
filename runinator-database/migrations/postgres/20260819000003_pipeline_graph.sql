ALTER TABLE pipelines ADD COLUMN IF NOT EXISTS graph TEXT NOT NULL DEFAULT '{"version":0,"members":[],"links":[],"joins":{}}';
ALTER TABLE pipelines ADD COLUMN IF NOT EXISTS concurrency TEXT NOT NULL DEFAULT '{"max_concurrent_runs":0,"on_conflict":"allow"}';

CREATE TABLE IF NOT EXISTS pipeline_member_attempts (
    id UUID PRIMARY KEY,
    pipeline_run_id UUID NOT NULL REFERENCES pipeline_runs(id) ON DELETE CASCADE,
    member_key TEXT NOT NULL,
    workflow_id UUID NOT NULL,
    attempt BIGINT NOT NULL,
    workflow_run_id UUID NULL REFERENCES workflow_runs(id),
    status TEXT NOT NULL,
    parameters TEXT NOT NULL DEFAULT '{}',
    result TEXT NOT NULL DEFAULT 'null',
    message TEXT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    UNIQUE (pipeline_run_id, member_key, attempt),
    UNIQUE (workflow_run_id)
);
CREATE INDEX IF NOT EXISTS idx_pipeline_member_attempts_run ON pipeline_member_attempts(pipeline_run_id, member_key, attempt);

DELETE FROM workflow_triggers WHERE (configuration::jsonb ? 'pipeline_id');
UPDATE pipelines SET metadata = jsonb_set(COALESCE(NULLIF(metadata, ''), '{}')::jsonb, '{requires_reimport}', 'true'::jsonb)::text;
UPDATE pipeline_runs SET status = 'failed', finished_at = EXTRACT(EPOCH FROM NOW())::BIGINT, message = 'Pipeline graph upgrade requires source pack reimport'
WHERE status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled');
