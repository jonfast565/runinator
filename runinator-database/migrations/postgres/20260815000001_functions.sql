-- packaged functions: immutable code published to the platform and invoked as ordinary actions.
--
-- the shape is a package owning versions, a version owning exports, and an alias pointing at a
-- version. only the alias moves. a compiled workflow records the version and artifact digest it was
-- built against, so promoting an alias changes what new calls resolve to and nothing else.

-- the bytes, addressed by content. keyed by digest rather than by an id, which is what makes
-- republishing identical bytes free and lets two machines agree on the artifact for one source tree.
CREATE TABLE IF NOT EXISTS function_artifacts (
    digest TEXT PRIMARY KEY,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    media_type TEXT NOT NULL DEFAULT 'application/zip',
    created_at BIGINT NOT NULL
);

-- the named unit that owns versions and aliases. carries no code itself, so grants and aliases have
-- something stable to point at while versions come and go.
CREATE TABLE IF NOT EXISTS function_packages (
    id UUID PRIMARY KEY,
    org_id UUID NULL,
    namespace TEXT NULL,
    name TEXT NOT NULL,
    identity_key TEXT NOT NULL,
    description TEXT NULL,
    latest_version BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- identity is (org, namespace, name), but two of those are nullable and every engine here treats
-- NULLs as distinct in a unique index — so a platform-global package could otherwise be created
-- twice under one name. the store renders the triple into `identity_key` and the uniqueness lives
-- there: one plain index that means the same thing on sqlite, postgres, and mysql, rather than a
-- functional index postgres allows and mariadb does not.
CREATE UNIQUE INDEX IF NOT EXISTS idx_function_packages_identity
    ON function_packages(identity_key);
CREATE INDEX IF NOT EXISTS idx_function_packages_org ON function_packages(org_id);

-- one immutable release. never updated after insert: a workflow revision that pinned this version
-- must keep meaning what it meant.
CREATE TABLE IF NOT EXISTS function_versions (
    id UUID PRIMARY KEY,
    package_id UUID NOT NULL REFERENCES function_packages(id) ON DELETE CASCADE,
    version BIGINT NOT NULL,
    artifact_digest TEXT NOT NULL REFERENCES function_artifacts(digest),
    manifest TEXT NOT NULL DEFAULT '{}',
    runtime TEXT NOT NULL DEFAULT '{}',
    published_by UUID NULL,
    created_at BIGINT NOT NULL
);

-- the version number is how a binding names its code, so a duplicate must be impossible rather than
-- unlikely: two racing publishes lose one insert instead of forking what "version 3" means.
CREATE UNIQUE INDEX IF NOT EXISTS idx_function_versions_seq
    ON function_versions(package_id, version);
CREATE INDEX IF NOT EXISTS idx_function_versions_artifact
    ON function_versions(artifact_digest);

-- one callable entry point with a typed signature.
CREATE TABLE IF NOT EXISTS function_exports (
    id UUID PRIMARY KEY,
    version_id UUID NOT NULL REFERENCES function_versions(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    handler TEXT NOT NULL,
    description TEXT NULL,
    input TEXT NOT NULL DEFAULT '[]',
    output TEXT NOT NULL DEFAULT '[]',
    limits TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_function_exports_name
    ON function_exports(version_id, name);

-- the one movable pointer at a version.
CREATE TABLE IF NOT EXISTS function_aliases (
    id UUID PRIMARY KEY,
    package_id UUID NOT NULL REFERENCES function_packages(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    version_id UUID NOT NULL REFERENCES function_versions(id),
    version BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_function_aliases_name
    ON function_aliases(package_id, name);
CREATE INDEX IF NOT EXISTS idx_function_aliases_version
    ON function_aliases(version_id);

-- the hidden workflow generated per export so a direct http invocation runs the same reducer path a
-- workflow call does, rather than a second execution engine.
CREATE TABLE IF NOT EXISTS function_adapter_workflows (
    id UUID PRIMARY KEY,
    export_id UUID NOT NULL REFERENCES function_exports(id) ON DELETE CASCADE,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_function_adapter_export
    ON function_adapter_workflows(export_id);
CREATE INDEX IF NOT EXISTS idx_function_adapter_workflow
    ON function_adapter_workflows(workflow_id);
