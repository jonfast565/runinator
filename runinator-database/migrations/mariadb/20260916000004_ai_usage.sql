CREATE TABLE IF NOT EXISTS workflow_ai_usage (
    event_id BINARY(16) PRIMARY KEY,
    effect_id BINARY(16) NOT NULL,
    workflow_run_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    node_id TEXT NULL,
    attempt BIGINT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens BIGINT NOT NULL,
    cached_input_tokens BIGINT NOT NULL,
    cache_creation_input_tokens BIGINT NOT NULL,
    output_tokens BIGINT NOT NULL,
    reasoning_tokens BIGINT NOT NULL,
    cost_microusd BIGINT NULL,
    cost_source TEXT NULL,
    recorded_at BIGINT NOT NULL,
    CONSTRAINT fk_workflow_ai_usage_effect FOREIGN KEY (effect_id) REFERENCES workflow_effects(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_ai_usage_run FOREIGN KEY (workflow_run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_ai_usage_workflow FOREIGN KEY (workflow_id) REFERENCES workflows(id) ON DELETE CASCADE
);
CREATE INDEX idx_workflow_ai_usage_run ON workflow_ai_usage(workflow_run_id, recorded_at);
CREATE INDEX idx_workflow_ai_usage_workflow ON workflow_ai_usage(workflow_id, recorded_at);
CREATE INDEX idx_workflow_ai_usage_effect ON workflow_ai_usage(effect_id);
