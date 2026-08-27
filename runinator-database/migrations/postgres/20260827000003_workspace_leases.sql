CREATE TABLE IF NOT EXISTS workspace_leases (
    id UUID PRIMARY KEY,
    admission_id UUID NOT NULL REFERENCES ingress_admissions(id) ON DELETE CASCADE,
    generation BIGINT NOT NULL,
    scope TEXT NOT NULL,
    attempt BIGINT NOT NULL,
    worker_instance_id TEXT NOT NULL,
    worker_replica_id UUID NULL,
    local_key TEXT NOT NULL,
    requirements TEXT NOT NULL,
    status TEXT NOT NULL,
    version BIGINT NOT NULL,
    leased_until BIGINT NOT NULL,
    unavailable_since BIGINT NULL,
    evidence TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE(admission_id, generation, scope, attempt)
);
CREATE INDEX IF NOT EXISTS idx_workspace_leases_worker ON workspace_leases(worker_instance_id, status);
CREATE INDEX IF NOT EXISTS idx_workspace_leases_expiry ON workspace_leases(status, leased_until);
