-- organizations (tenants) and per-user memberships. a user belongs to many orgs, each with a role.
CREATE TABLE IF NOT EXISTS organizations (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    disabled BOOLEAN NOT NULL,
    max_nodes_json TEXT NOT NULL DEFAULT '{}',
    max_monthly_cents BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
