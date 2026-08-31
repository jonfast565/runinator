-- A kick ends one activation without revoking the permanent machine enrollment behind it.
ALTER TABLE replicas ADD COLUMN kicked_at INTEGER NULL;
CREATE INDEX IF NOT EXISTS idx_replicas_owner_kicked
    ON replicas(registered_by_principal_id, kicked_at);

-- Enrollment grants are timed unless the issuer explicitly requests permanent machine access.
ALTER TABLE agent_enrollment_tokens ADD COLUMN permanent INTEGER NOT NULL DEFAULT 0;
