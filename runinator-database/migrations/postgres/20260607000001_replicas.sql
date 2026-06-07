CREATE TABLE IF NOT EXISTS replicas (
    replica_id BIGSERIAL PRIMARY KEY,
    replica_type TEXT NOT NULL,
    instance_id TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'live',
    display_name TEXT NULL,
    host TEXT NULL,
    port INTEGER NULL,
    base_path TEXT NULL,
    observed_ip TEXT NULL,
    attributes TEXT NOT NULL DEFAULT '{}',
    first_seen_at BIGINT NOT NULL,
    last_heartbeat_at BIGINT NOT NULL,
    last_seen_at BIGINT NOT NULL,
    offline_at BIGINT NULL,
    UNIQUE(instance_id, runtime_id)
);

CREATE TABLE IF NOT EXISTS replica_provider_registrations (
    replica_id BIGINT NOT NULL REFERENCES replicas(replica_id) ON DELETE CASCADE,
    provider_name TEXT NOT NULL,
    provider_json TEXT NOT NULL DEFAULT '{}',
    first_registered_at BIGINT NOT NULL,
    last_registered_at BIGINT NOT NULL,
    last_heartbeat_at BIGINT NOT NULL,
    PRIMARY KEY (replica_id, provider_name)
);

ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS trigger_source_kind TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS trigger_actor_type TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS trigger_actor_replica_id BIGINT NULL REFERENCES replicas(replica_id);
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS trigger_actor_display_name TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS trigger_request_host TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS trigger_request_ip TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS trigger_metadata TEXT NOT NULL DEFAULT '{}';

ALTER TABLE workflow_node_runs ADD COLUMN IF NOT EXISTS current_executor_replica_id BIGINT NULL REFERENCES replicas(replica_id);
ALTER TABLE workflow_node_runs ADD COLUMN IF NOT EXISTS last_executor_replica_id BIGINT NULL REFERENCES replicas(replica_id);
ALTER TABLE workflow_node_runs ADD COLUMN IF NOT EXISTS executor_claimed_at BIGINT NULL;
ALTER TABLE workflow_node_runs ADD COLUMN IF NOT EXISTS executor_released_at BIGINT NULL;

CREATE INDEX IF NOT EXISTS idx_replicas_type_status ON replicas(replica_type, status, last_heartbeat_at);
CREATE INDEX IF NOT EXISTS idx_replicas_instance ON replicas(instance_id, runtime_id);
CREATE INDEX IF NOT EXISTS idx_replica_provider_registrations_replica ON replica_provider_registrations(replica_id);
