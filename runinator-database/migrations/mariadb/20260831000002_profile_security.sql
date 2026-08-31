ALTER TABLE auth_sessions ADD COLUMN last_seen_at BIGINT NOT NULL DEFAULT 0;
ALTER TABLE auth_sessions ADD COLUMN user_agent TEXT NULL;
ALTER TABLE auth_sessions ADD COLUMN ip_address VARCHAR(64) NULL;

UPDATE auth_sessions SET last_seen_at = created_at WHERE last_seen_at = 0;
