ALTER TABLE function_packages ADD COLUMN IF NOT EXISTS archived_at BIGINT NULL;
CREATE INDEX IF NOT EXISTS idx_function_packages_archived ON function_packages(archived_at);
