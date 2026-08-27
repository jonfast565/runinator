CREATE TABLE IF NOT EXISTS workspace_leases (
    id BLOB PRIMARY KEY,
    admission_id BLOB NOT NULL,
    generation INTEGER NOT NULL,
    scope TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    worker_instance_id TEXT NOT NULL,
    worker_replica_id BLOB NULL,
    local_key TEXT NOT NULL,
    requirements TEXT NOT NULL,
    status TEXT NOT NULL,
    version INTEGER NOT NULL,
    leased_until INTEGER NOT NULL,
    unavailable_since INTEGER NULL,
    evidence TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(admission_id) REFERENCES ingress_admissions(id) ON DELETE CASCADE,
    UNIQUE(admission_id, generation, scope, attempt)
);
CREATE INDEX IF NOT EXISTS idx_workspace_leases_worker ON workspace_leases(worker_instance_id, status);
CREATE INDEX IF NOT EXISTS idx_workspace_leases_expiry ON workspace_leases(status, leased_until);
