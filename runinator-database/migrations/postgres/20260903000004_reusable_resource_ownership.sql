ALTER TABLE resource_ownership DROP CONSTRAINT resource_ownership_resource_type_check;
ALTER TABLE resource_ownership ADD CONSTRAINT resource_ownership_resource_type_check
    CHECK (resource_type IN ('workflow', 'pipeline', 'function_package', 'console_session', 'setting', 'execution_profile', 'orchestration_adapter', 'library_file', 'notification_policy'));

INSERT INTO resource_ownership SELECT 'setting', id,
    CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
    CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
    NULL, 1, updated_at, updated_at FROM settings;
INSERT INTO resource_ownership SELECT 'execution_profile', id,
    CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
    CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
    NULL, 1, created_at, updated_at FROM execution_profiles;
INSERT INTO resource_ownership SELECT 'orchestration_adapter', id, 'organization', org_id,
    'organization', org_id, NULL, 1, created_at, updated_at FROM orchestration_adapters;
INSERT INTO resource_ownership SELECT 'library_file', id, 'organization', org_id,
    CASE WHEN owner_id IS NULL THEN 'organization' ELSE 'user' END,
    COALESCE(owner_id, org_id), owner_id, 1, created_at, created_at
    FROM workflow_files WHERE scope = 'library' AND org_id IS NOT NULL;
INSERT INTO resource_ownership SELECT 'notification_policy', id,
    CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
    CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
    NULL, 1, created_at, updated_at FROM notification_policies WHERE workflow_id IS NULL;
