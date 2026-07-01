-- per-org spending/scale quotas and an append-only usage ledger sampled from live node counts.
CREATE TABLE IF NOT EXISTS org_quotas (
    org_id UUID PRIMARY KEY,
    max_nodes_json TEXT NOT NULL DEFAULT '{}',
    max_monthly_cents BIGINT NOT NULL DEFAULT 0,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS org_usage_ledger (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    backend TEXT NOT NULL,
    kind TEXT NOT NULL,
    node_count BIGINT NOT NULL,
    sampled_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_org_usage_org_time ON org_usage_ledger(org_id, sampled_at);

CREATE TABLE IF NOT EXISTS org_resource_groups (
    org_id UUID NOT NULL,
    backend TEXT NOT NULL,
    kind TEXT NOT NULL,
    desired BIGINT NOT NULL,
    dedicated BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, backend, kind)
);
