-- make usage sampling idempotent per (org, backend, kind, window). the sampler now buckets
-- sampled_at to the interval boundary and inserts a no-op ON DUPLICATE KEY UPDATE, so any number of
-- ws replicas / background workers sampling the same window converge to one row instead of
-- over-counting node-hours by the instance count. existing rows carry distinct (unbucketed)
-- sampled_at values, so the unique index builds cleanly on current data.
CREATE UNIQUE INDEX idx_org_usage_unique
    ON org_usage_ledger(org_id, backend, kind, sampled_at);
