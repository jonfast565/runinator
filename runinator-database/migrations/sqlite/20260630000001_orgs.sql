-- organizations (tenants) and per-user memberships. a user belongs to many orgs, each with a role.
CREATE TABLE IF NOT EXISTS organizations (
    id BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    disabled BOOL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS org_memberships (
    org_id BLOB NOT NULL,
    user_id BLOB NOT NULL,
    role TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (org_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_org_memberships_user ON org_memberships(user_id);
