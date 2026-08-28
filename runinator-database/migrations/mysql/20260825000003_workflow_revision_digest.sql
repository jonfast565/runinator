-- Exact revision pins verify this digest as well as the per-workflow sequence number. Existing
-- history remains intentionally unpinned; an empty value is distinguishable from a verified
-- sha-256 digest and can never satisfy a new pin.
-- expression default, not a literal: mysql 8 rejects a literal default on a TEXT column and would
-- stop this migration and every one after it. see 20260607000001_replicas.sql.
ALTER TABLE workflow_revisions ADD COLUMN digest TEXT NOT NULL DEFAULT ('');
