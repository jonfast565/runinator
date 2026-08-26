ALTER TABLE workflows ADD COLUMN resource_key TEXT NULL;
UPDATE workflows SET resource_key = name WHERE resource_key IS NULL;
CREATE INDEX IF NOT EXISTS idx_workflows_resource_key ON workflows(resource_key);

ALTER TABLE pipelines ADD COLUMN resource_key TEXT NULL;
ALTER TABLE pipelines ADD COLUMN namespace TEXT NULL;
UPDATE pipelines SET resource_key = name WHERE resource_key IS NULL;
CREATE INDEX IF NOT EXISTS idx_pipelines_resource_key ON pipelines(resource_key);
