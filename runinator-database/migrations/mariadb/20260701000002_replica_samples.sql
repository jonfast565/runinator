-- lightweight time-series of replica resource telemetry, kept separate from the replica row (whose
-- attributes only hold the latest snapshot) so the ui can draw historical sparklines. the numeric
-- fields ride in a json `data` blob (matching the codebase convention for structured columns);
-- replica_id/sampled_at are typed for indexing and pruning. pruned by a retention window.
CREATE TABLE IF NOT EXISTS replica_samples (
    id BINARY(16) PRIMARY KEY,
    replica_id BINARY(16) NOT NULL,
    sampled_at BIGINT NOT NULL,
    data TEXT NOT NULL,
    CONSTRAINT fk_replica_samples_replica FOREIGN KEY (replica_id) REFERENCES replicas(replica_id) ON DELETE CASCADE
);
CREATE INDEX idx_replica_samples_replica ON replica_samples(replica_id, sampled_at);
