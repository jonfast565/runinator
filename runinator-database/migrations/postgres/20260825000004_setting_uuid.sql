-- Derive a deterministic one-time UUID from the existing logical path so no extension is required
-- during migration. New writes mint UUIDv7 values in the application.
ALTER TABLE settings ADD COLUMN IF NOT EXISTS id UUID NULL;
UPDATE settings
SET id = (md5(kind || ':' || scope || ':' || name))::uuid
WHERE id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_settings_id ON settings(id);
