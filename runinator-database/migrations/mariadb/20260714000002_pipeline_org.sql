-- attribute a pipeline to an owning organization (tenant). nullable so unqualified pipelines stay
-- platform-global; visibility is scoped to the owning org (mirrors workflows).
ALTER TABLE pipelines ADD COLUMN org_id BINARY(16) NULL;
CREATE INDEX idx_pipelines_org ON pipelines(org_id);
