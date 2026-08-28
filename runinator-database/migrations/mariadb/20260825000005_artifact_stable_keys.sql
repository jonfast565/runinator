ALTER TABLE workflows ADD COLUMN resource_key VARCHAR(512) NULL;
UPDATE workflows SET resource_key = name WHERE resource_key IS NULL;
CREATE INDEX idx_workflows_resource_key ON workflows(resource_key);

ALTER TABLE pipelines ADD COLUMN resource_key VARCHAR(512) NULL;
ALTER TABLE pipelines ADD COLUMN namespace VARCHAR(512) NULL;
UPDATE pipelines SET resource_key = name WHERE resource_key IS NULL;
CREATE INDEX idx_pipelines_resource_key ON pipelines(resource_key);
