-- Clean-cut hierarchical RBAC. Legacy service credentials are intentionally revoked.
CREATE TABLE role_assignments (
    principal_kind VARCHAR(32) NOT NULL,
    principal_id BINARY(16) NOT NULL,
    scope_kind VARCHAR(32) NOT NULL,
    scope_id BINARY(16) NULL,
    scope_key VARCHAR(96) NOT NULL,
    role_kind VARCHAR(32) NOT NULL,
    role VARCHAR(32) NOT NULL,
    created_by BINARY(16) NULL,
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
FROM users WHERE disabled = 0;

INSERT INTO role_assignments
SELECT 'user', user_id, 'organization', org_id, CONCAT('organization:', LOWER(CONCAT(
       SUBSTR(HEX(org_id),1,8),'-',SUBSTR(HEX(org_id),9,4),'-',SUBSTR(HEX(org_id),13,4),'-',SUBSTR(HEX(org_id),17,4),'-',SUBSTR(HEX(org_id),21,12)))),
       'organization', role, NULL, created_at, created_at
FROM org_memberships;

CREATE TABLE service_accounts (
    id BINARY(16) PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    disabled TINYINT(1) NOT NULL,
    created_by BINARY(16) NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

DROP TABLE api_keys;
CREATE TABLE api_keys (
    id BINARY(16) PRIMARY KEY,
    name TEXT NOT NULL,
    principal_kind VARCHAR(32) NOT NULL,
    principal_id BINARY(16) NOT NULL,
    system_role VARCHAR(32) NULL,
    org_id BINARY(16) NULL,
    action_ceiling_json TEXT NULL,
    key_prefix VARCHAR(64) NOT NULL UNIQUE,
    key_hash TEXT NOT NULL,
    last_used_at BIGINT NULL,
    expires_at BIGINT NULL,
    disabled TINYINT(1) NOT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX idx_api_keys_principal ON api_keys(principal_kind, principal_id);
ALTER TABLE users DROP COLUMN is_admin;
ALTER TABLE teams ADD COLUMN scope_kind VARCHAR(32) NOT NULL DEFAULT 'platform';
ALTER TABLE teams ADD COLUMN scope_id BINARY(16) NULL;
ALTER TABLE team_members ADD COLUMN role VARCHAR(32) NOT NULL DEFAULT 'member';

-- MariaDB does not permit a CTE before UPDATE.  The derived-table form works on both MariaDB and
-- MySQL while keeping the candidate and unambiguous-owner checks in one statement.
UPDATE teams JOIN (
    SELECT candidates.team_id, MIN(candidates.org_id) AS org_id
    FROM (
        SELECT tm.team_id, om.org_id
        FROM team_members tm JOIN org_memberships om ON om.user_id = tm.user_id
        GROUP BY tm.team_id, om.org_id
        HAVING COUNT(DISTINCT tm.user_id) = (
            SELECT COUNT(*) FROM team_members all_tm WHERE all_tm.team_id = tm.team_id
        )
    ) AS candidates
    GROUP BY candidates.team_id
    HAVING COUNT(*) = 1
) AS unambiguous ON teams.id = unambiguous.team_id
SET teams.scope_kind = 'organization', teams.scope_id = unambiguous.org_id;

INSERT INTO role_assignments
SELECT 'user', user_id, 'team', team_id, CONCAT('team:', LOWER(CONCAT(
       SUBSTR(HEX(team_id),1,8),'-',SUBSTR(HEX(team_id),9,4),'-',SUBSTR(HEX(team_id),13,4),'-',SUBSTR(HEX(team_id),17,4),'-',SUBSTR(HEX(team_id),21,12)))),
       'team', role, NULL, UNIX_TIMESTAMP(), UNIX_TIMESTAMP()
FROM team_members;

CREATE TABLE resource_ownership (
    resource_type VARCHAR(64) NOT NULL,
    resource_id BINARY(16) NOT NULL,
    tenant_scope_kind VARCHAR(32) NOT NULL,
    tenant_scope_id BINARY(16) NULL,
    owner_scope_kind VARCHAR(32) NOT NULL,
    owner_scope_id BINARY(16) NULL,
    created_by BINARY(16) NULL,
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
