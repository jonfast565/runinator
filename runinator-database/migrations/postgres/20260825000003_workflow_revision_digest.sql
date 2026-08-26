-- Exact revision pins verify this digest as well as the per-workflow sequence number. Existing
-- history remains intentionally unpinned; an empty value is distinguishable from a verified
-- sha-256 digest and can never satisfy a new pin.
ALTER TABLE workflow_revisions ADD COLUMN IF NOT EXISTS digest TEXT NOT NULL DEFAULT '';
