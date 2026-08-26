-- A setting's path is an alias, not its durable identity. Existing rows receive a logical UUID
-- once; later writes preserve it through the `(kind, scope, name)` conflict update.
ALTER TABLE settings ADD COLUMN id BLOB NULL;
UPDATE settings SET id = randomblob(16) WHERE id IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_settings_id ON settings(id);
