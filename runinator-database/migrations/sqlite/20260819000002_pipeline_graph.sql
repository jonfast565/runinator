ALTER TABLE pipelines ADD COLUMN graph TEXT NOT NULL DEFAULT '{"version":0,"members":[],"links":[],"joins":{}}';
ALTER TABLE pipelines ADD COLUMN concurrency TEXT NOT NULL DEFAULT '{"max_concurrent_runs":0,"on_conflict":"allow"}';

CREATE TABLE pipeline_member_attempts (
    id BLOB PRIMARY KEY,
    pipeline_run_id BLOB NOT NULL REFERENCES pipeline_runs(id) ON DELETE CASCADE,
    member_key TEXT NOT NULL,
    workflow_id BLOB NOT NULL,
    attempt INTEGER NOT NULL,
    workflow_run_id BLOB NULL REFERENCES workflow_runs(id),
    status TEXT NOT NULL,
    parameters TEXT NOT NULL DEFAULT '{}',
    result TEXT NOT NULL DEFAULT 'null',
    message TEXT NULL,
    created_at INTEGER NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL,
    UNIQUE (pipeline_run_id, member_key, attempt),
    UNIQUE (workflow_run_id)
);
CREATE INDEX idx_pipeline_member_attempts_run ON pipeline_member_attempts(pipeline_run_id, member_key, attempt);

DELETE FROM workflow_triggers WHERE json_extract(configuration, '$.pipeline_id') IS NOT NULL;
UPDATE pipelines SET metadata = json_set(COALESCE(NULLIF(metadata, ''), '{}'), '$.requires_reimport', json('true'));
UPDATE pipeline_runs SET status = 'failed', finished_at = unixepoch(), message = 'Pipeline graph upgrade requires source pack reimport'
WHERE status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled');
