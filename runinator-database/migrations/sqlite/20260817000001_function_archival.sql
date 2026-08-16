ALTER TABLE function_packages ADD COLUMN archived_at INTEGER NULL;
CREATE INDEX IF NOT EXISTS idx_function_packages_archived ON function_packages(archived_at);
