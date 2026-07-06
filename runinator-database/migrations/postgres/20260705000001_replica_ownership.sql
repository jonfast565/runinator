-- who registered this replica, so a lower-trust external caller (e.g. a desktop-agent connecting
-- through the ws broker relay) can be checked against the replica_id/labels it presents. populated
-- once at insert and left untouched by later heartbeats/upserts, so ownership can't be reassigned.
ALTER TABLE replicas ADD COLUMN IF NOT EXISTS registered_by_principal_id UUID NULL;
ALTER TABLE replicas ADD COLUMN IF NOT EXISTS registered_by_kind TEXT NULL;
ALTER TABLE replicas ADD COLUMN IF NOT EXISTS registered_by_org_id UUID NULL;
