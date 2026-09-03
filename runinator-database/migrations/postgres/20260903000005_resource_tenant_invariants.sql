-- A global resource belongs to the platform. Individual and team authority over it is expressed
-- through a grant, never by changing the resource's tenant or owner scope.
INSERT INTO resource_grants (id, resource_type, resource_id, principal_type, principal_id, permission, created_at)
SELECT md5(ownership.resource_type || ':' || ownership.resource_id::text || ':' || ownership.owner_scope_kind || ':' || ownership.owner_scope_id::text)::uuid,
       ownership.resource_type, ownership.resource_id, ownership.owner_scope_kind,
       ownership.owner_scope_id, 'own', ownership.updated_at
FROM resource_ownership ownership
WHERE ownership.tenant_scope_kind = 'platform'
  AND ownership.owner_scope_kind IN ('user', 'team')
ON CONFLICT (resource_type, resource_id, principal_type, principal_id)
DO UPDATE SET permission = EXCLUDED.permission,
              created_at = EXCLUDED.created_at;
UPDATE resource_ownership
SET owner_scope_kind = 'platform', owner_scope_id = NULL, authz_version = authz_version + 1
WHERE tenant_scope_kind = 'platform' AND owner_scope_kind <> 'platform';

ALTER TABLE resource_ownership
    ADD CONSTRAINT resource_ownership_tenant_scope_check
    CHECK (tenant_scope_kind IN ('platform', 'organization'));
ALTER TABLE resource_ownership
    ADD CONSTRAINT resource_ownership_platform_owner_check
    CHECK (tenant_scope_kind <> 'platform' OR owner_scope_kind = 'platform');
