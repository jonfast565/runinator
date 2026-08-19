-- Clean-cut hierarchical RBAC. Legacy service credentials are intentionally revoked.
CREATE TABLE role_assignments (
    principal_kind TEXT NOT NULL,
    principal_id UUID NOT NULL,
    scope_kind TEXT NOT NULL,
    scope_id UUID NULL,
    scope_key TEXT NOT NULL,
    role_kind TEXT NOT NULL,
    role TEXT NOT NULL,
    created_by UUID NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CHECK (principal_kind IN ('user', 'service')),
    CHECK (scope_kind IN ('platform', 'organization', 'team')),
    CHECK ((scope_kind = 'platform' AND scope_id IS NULL) OR (scope_kind <> 'platform' AND scope_id IS NOT NULL)),
    CHECK ((role_kind = 'platform' AND role IN ('member', 'auditor', 'operator', 'admin'))
        OR (role_kind IN ('organization', 'team') AND role IN ('member', 'operator', 'admin', 'owner'))),
    CHECK (role_kind = scope_kind),
    PRIMARY KEY (principal_kind, principal_id, scope_key)
);
CREATE INDEX idx_role_assignments_principal ON role_assignments(principal_kind, principal_id);
CREATE INDEX idx_role_assignments_scope ON role_assignments(scope_key);

INSERT INTO role_assignments
SELECT 'user', id, 'platform', NULL, 'platform', 'platform',
       CASE WHEN is_admin THEN 'admin' ELSE 'member' END, NULL, created_at, updated_at
FROM users WHERE disabled = FALSE;

INSERT INTO role_assignments
SELECT 'user', user_id, 'organization', org_id, 'organization:' || org_id::text,
       'organization', role, NULL, created_at, created_at
FROM org_memberships;

CREATE TABLE service_accounts (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    disabled BOOLEAN NOT NULL,
    created_by UUID NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

DROP TABLE api_keys;
CREATE TABLE api_keys (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    principal_kind TEXT NOT NULL,
    principal_id UUID NOT NULL,
    system_role TEXT NULL,
    org_id UUID NULL,
    action_ceiling_json TEXT NULL,
    key_prefix TEXT NOT NULL UNIQUE,
    key_hash TEXT NOT NULL,
    last_used_at BIGINT NULL,
    expires_at BIGINT NULL,
    disabled BOOLEAN NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX idx_api_keys_principal ON api_keys(principal_kind, principal_id);
ALTER TABLE users DROP COLUMN is_admin;
ALTER TABLE teams ADD COLUMN scope_kind TEXT NOT NULL DEFAULT 'platform';
ALTER TABLE teams ADD COLUMN scope_id UUID NULL;
ALTER TABLE team_members ADD COLUMN role TEXT NOT NULL DEFAULT 'member';

WITH candidates AS (
    SELECT tm.team_id, om.org_id
    FROM team_members tm JOIN org_memberships om ON om.user_id = tm.user_id
    GROUP BY tm.team_id, om.org_id
    HAVING COUNT(DISTINCT tm.user_id) = (SELECT COUNT(*) FROM team_members all_tm WHERE all_tm.team_id = tm.team_id)
), unambiguous AS (
    SELECT team_id, MIN(org_id::text)::uuid AS org_id FROM candidates GROUP BY team_id HAVING COUNT(*) = 1
)
UPDATE teams SET scope_kind = 'organization', scope_id = unambiguous.org_id
FROM unambiguous WHERE teams.id = unambiguous.team_id;

INSERT INTO role_assignments
SELECT 'user', user_id, 'team', team_id, 'team:' || team_id::text,
       'team', role, NULL, EXTRACT(EPOCH FROM NOW())::BIGINT, EXTRACT(EPOCH FROM NOW())::BIGINT
FROM team_members;

CREATE TABLE resource_ownership (
    resource_type TEXT NOT NULL,
    resource_id UUID NOT NULL,
    tenant_scope_kind TEXT NOT NULL,
    tenant_scope_id UUID NULL,
    owner_scope_kind TEXT NOT NULL,
    owner_scope_id UUID NULL,
    created_by UUID NULL,
    authz_version BIGINT NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (resource_type, resource_id),
    CHECK (resource_type IN ('workflow', 'pipeline', 'function_package', 'console_session')),
    CHECK (tenant_scope_kind IN ('platform', 'organization', 'team', 'user')),
    CHECK (owner_scope_kind IN ('platform', 'organization', 'team', 'user')),
    CHECK ((tenant_scope_kind = 'platform' AND tenant_scope_id IS NULL) OR (tenant_scope_kind <> 'platform' AND tenant_scope_id IS NOT NULL)),
    CHECK ((owner_scope_kind = 'platform' AND owner_scope_id IS NULL) OR (owner_scope_kind <> 'platform' AND owner_scope_id IS NOT NULL))
);
CREATE INDEX idx_resource_ownership_owner ON resource_ownership(owner_scope_kind, owner_scope_id);
CREATE INDEX idx_resource_ownership_tenant ON resource_ownership(tenant_scope_kind, tenant_scope_id);

INSERT INTO resource_ownership
SELECT 'workflow', w.id,
       CASE WHEN w.org_id IS NULL THEN 'platform' ELSE 'organization' END, w.org_id,
       CASE WHEN g.principal_id IS NOT NULL THEN 'user' WHEN w.org_id IS NOT NULL THEN 'organization' ELSE 'platform' END,
       COALESCE(g.principal_id, w.org_id), g.principal_id, 1, w.created_at, w.updated_at
FROM workflows w LEFT JOIN resource_grants g ON g.id = (SELECT g2.id FROM resource_grants g2
    WHERE g2.resource_type = 'workflow' AND g2.resource_id = w.id
      AND g2.principal_type = 'user' AND g2.permission = 'own'
    ORDER BY g2.created_at, g2.id LIMIT 1);

INSERT INTO resource_ownership
SELECT 'pipeline', p.id,
       CASE WHEN p.org_id IS NULL THEN 'platform' ELSE 'organization' END, p.org_id,
       CASE WHEN g.principal_id IS NOT NULL THEN 'user' WHEN p.org_id IS NOT NULL THEN 'organization' ELSE 'platform' END,
       COALESCE(g.principal_id, p.org_id), g.principal_id, 1, p.created_at, p.updated_at
FROM pipelines p LEFT JOIN resource_grants g ON g.id = (SELECT g2.id FROM resource_grants g2
    WHERE g2.resource_type = 'pipeline' AND g2.resource_id = p.id
      AND g2.principal_type = 'user' AND g2.permission = 'own'
    ORDER BY g2.created_at, g2.id LIMIT 1);

INSERT INTO resource_ownership
SELECT 'function_package', id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       NULL, 1, created_at, updated_at
FROM function_packages;

INSERT INTO resource_ownership
SELECT 'console_session', id,
       CASE WHEN org_id IS NULL THEN 'platform' ELSE 'organization' END, org_id,
       CASE WHEN created_by IS NOT NULL THEN 'user' WHEN org_id IS NOT NULL THEN 'organization' ELSE 'platform' END,
       COALESCE(created_by, org_id), created_by, 1, created_at, updated_at
FROM console_sessions;
