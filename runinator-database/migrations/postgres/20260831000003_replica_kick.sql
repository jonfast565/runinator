-- A kick ends one activation without revoking the permanent machine enrollment behind it.
ALTER TABLE replicas ADD COLUMN IF NOT EXISTS kicked_at BIGINT NULL;
CREATE INDEX IF NOT EXISTS idx_replicas_owner_kicked
    ON replicas(registered_by_principal_id, kicked_at);

-- Enrollment grants are timed unless the issuer explicitly requests permanent machine access.
ALTER TABLE agent_enrollment_tokens ADD COLUMN IF NOT EXISTS permanent BOOLEAN NOT NULL DEFAULT FALSE;
