ALTER TABLE pipelines ADD COLUMN graph LONGTEXT NOT NULL DEFAULT ('{"version":0,"members":[],"links":[],"joins":{}}');
ALTER TABLE pipelines ADD COLUMN concurrency LONGTEXT NOT NULL DEFAULT ('{"max_concurrent_runs":0,"on_conflict":"allow"}');
ALTER TABLE pipelines MODIFY COLUMN workflow_ids LONGTEXT NULL;

CREATE TABLE pipeline_member_attempts (
    id BINARY(16) PRIMARY KEY,
    pipeline_run_id BINARY(16) NOT NULL,
    member_key VARCHAR(255) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    attempt BIGINT NOT NULL,
    workflow_run_id BINARY(16) NULL,
    status VARCHAR(32) NOT NULL,
    parameters LONGTEXT NOT NULL,
    result LONGTEXT NOT NULL,
    message TEXT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    CONSTRAINT fk_pipeline_member_attempt_run FOREIGN KEY (pipeline_run_id) REFERENCES pipeline_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_pipeline_member_attempt_workflow_run FOREIGN KEY (workflow_run_id) REFERENCES workflow_runs(id),
    UNIQUE KEY uq_pipeline_member_attempt (pipeline_run_id, member_key, attempt),
    UNIQUE KEY uq_pipeline_member_workflow_run (workflow_run_id),
    INDEX idx_pipeline_member_attempts_run (pipeline_run_id, member_key, attempt)
);

DELETE FROM workflow_triggers WHERE JSON_EXTRACT(configuration, '$.pipeline_id') IS NOT NULL;
UPDATE pipelines SET metadata = JSON_SET(COALESCE(NULLIF(metadata, ''), '{}'), '$.requires_reimport', TRUE);
UPDATE pipeline_runs SET status = 'failed', finished_at = UNIX_TIMESTAMP(), message = 'Pipeline graph upgrade requires source pack reimport'
WHERE status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled');
