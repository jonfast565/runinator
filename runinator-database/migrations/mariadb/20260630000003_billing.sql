-- per-org spending/scale quotas and an append-only usage ledger sampled from live node counts.
CREATE TABLE IF NOT EXISTS org_quotas (
    org_id BINARY(16) PRIMARY KEY,
    max_nodes_json TEXT NOT NULL,
    max_monthly_cents BIGINT NOT NULL DEFAULT 0,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS org_usage_ledger (
    id BINARY(16) PRIMARY KEY,
    org_id BINARY(16) NOT NULL,
    backend VARCHAR(32) NOT NULL,
    kind VARCHAR(32) NOT NULL,
    node_count BIGINT NOT NULL,
    sampled_at BIGINT NOT NULL
);
CREATE INDEX idx_org_usage_org_time ON org_usage_ledger(org_id, sampled_at);

CREATE TABLE IF NOT EXISTS org_resource_groups (
    org_id BINARY(16) NOT NULL,
    backend VARCHAR(32) NOT NULL,
    kind VARCHAR(32) NOT NULL,
    desired BIGINT NOT NULL,
    dedicated TINYINT(1) NOT NULL DEFAULT 1,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (org_id, backend, kind)
);
