CREATE TABLE IF NOT EXISTS workflow_ai_usage (
    event_id BLOB PRIMARY KEY,
    effect_id BLOB NOT NULL REFERENCES workflow_effects(id) ON DELETE CASCADE,
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    workflow_id BLOB NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    node_id TEXT NULL,
    attempt INTEGER NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    cached_input_tokens INTEGER NOT NULL,
    cache_creation_input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    reasoning_tokens INTEGER NOT NULL,
    cost_microusd INTEGER NULL,
    cost_source TEXT NULL,
    recorded_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflow_ai_usage_run ON workflow_ai_usage(workflow_run_id, recorded_at);
CREATE INDEX IF NOT EXISTS idx_workflow_ai_usage_workflow ON workflow_ai_usage(workflow_id, recorded_at);
CREATE INDEX IF NOT EXISTS idx_workflow_ai_usage_effect ON workflow_ai_usage(effect_id);
