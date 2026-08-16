ALTER TABLE function_packages ADD COLUMN archived_at BIGINT NULL;
CREATE INDEX idx_function_packages_archived ON function_packages(archived_at);
