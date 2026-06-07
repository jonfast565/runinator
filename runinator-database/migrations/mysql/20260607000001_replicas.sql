CREATE TABLE IF NOT EXISTS replicas (
    replica_id BINARY(16) PRIMARY KEY,
    replica_type TEXT NOT NULL,
    instance_id TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'live',
    display_name TEXT NULL,
    host TEXT NULL,
    port BIGINT NULL,
    base_path TEXT NULL,
    observed_ip TEXT NULL,
    attributes TEXT NOT NULL,
    first_seen_at BIGINT NOT NULL,
    last_heartbeat_at BIGINT NOT NULL,
    last_seen_at BIGINT NOT NULL,
    offline_at BIGINT NULL,
    UNIQUE KEY uq_replicas_instance_runtime (instance_id(191), runtime_id(191))
);

CREATE TABLE IF NOT EXISTS replica_provider_registrations (
    replica_id BINARY(16) NOT NULL,
    provider_name TEXT NOT NULL,
    provider_json TEXT NOT NULL,
    first_registered_at BIGINT NOT NULL,
    last_registered_at BIGINT NOT NULL,
    last_heartbeat_at BIGINT NOT NULL,
    PRIMARY KEY (replica_id, provider_name(191)),
    CONSTRAINT fk_replica_provider_registrations_replica FOREIGN KEY (replica_id) REFERENCES replicas(replica_id) ON DELETE CASCADE
);

ALTER TABLE workflow_runs ADD COLUMN trigger_source_kind TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN trigger_actor_type TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN trigger_actor_replica_id BINARY(16) NULL;
ALTER TABLE workflow_runs ADD COLUMN trigger_actor_display_name TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN trigger_request_host TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN trigger_request_ip TEXT NULL;
ALTER TABLE workflow_runs ADD COLUMN trigger_metadata TEXT NOT NULL DEFAULT '{}';

ALTER TABLE workflow_node_runs ADD COLUMN current_executor_replica_id BINARY(16) NULL;
ALTER TABLE workflow_node_runs ADD COLUMN last_executor_replica_id BINARY(16) NULL;
ALTER TABLE workflow_node_runs ADD COLUMN executor_claimed_at BIGINT NULL;
ALTER TABLE workflow_node_runs ADD COLUMN executor_released_at BIGINT NULL;

CREATE INDEX idx_replicas_type_status ON replicas(replica_type(32), status(32), last_heartbeat_at);
CREATE INDEX idx_replicas_instance ON replicas(instance_id(191), runtime_id(191));
CREATE INDEX idx_replica_provider_registrations_replica ON replica_provider_registrations(replica_id);
