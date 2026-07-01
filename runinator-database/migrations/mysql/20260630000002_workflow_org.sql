-- attribute a workflow to an owning organization (tenant). nullable so existing unqualified
-- workflows stay platform-global; a workflow's runs inherit its org.
ALTER TABLE workflows ADD COLUMN org_id BINARY(16) NULL;
CREATE INDEX idx_workflows_org ON workflows(org_id);
