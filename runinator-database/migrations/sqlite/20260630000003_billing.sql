-- per-org spending/scale quotas and an append-only usage ledger sampled from live node counts.
CREATE TABLE IF NOT EXISTS org_quotas (
    org_id BLOB PRIMARY KEY,
    max_nodes_json TEXT NOT NULL DEFAULT '{}',
    max_monthly_cents INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS org_usage_ledger (
    id BLOB PRIMARY KEY,
    org_id BLOB NOT NULL,
    backend TEXT NOT NULL,
    kind TEXT NOT NULL,
    node_count INTEGER NOT NULL,
    sampled_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_org_usage_org_time ON org_usage_ledger(org_id, sampled_at);

CREATE TABLE IF NOT EXISTS org_resource_groups (
    org_id BLOB NOT NULL,
    backend TEXT NOT NULL,
    kind TEXT NOT NULL,
    desired INTEGER NOT NULL,
    dedicated BOOL NOT NULL DEFAULT 1,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (org_id, backend, kind)
);
