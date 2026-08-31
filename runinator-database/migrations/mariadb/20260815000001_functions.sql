-- packaged functions: immutable code published to the platform and invoked as ordinary actions.
--
-- the shape is a package owning versions, a version owning exports, and an alias pointing at a
-- version. only the alias moves. a compiled workflow records the version and artifact digest it was
-- built against, so promoting an alias changes what new calls resolve to and nothing else.
--
-- three mysql-specific shapes, all forced rather than chosen:
--
--  * every indexed string is VARCHAR, because mysql cannot index a TEXT column without a prefix;
--  * the json-ish columns carry no DEFAULT, because mysql 8 rejects a literal default on TEXT;
--  * foreign keys are declared as table-level constraints rather than inline `REFERENCES` clauses.
--    the inline form is the divergence that matters here: **mariadb creates a real foreign key from
--    it, mysql 8 parses and silently discards it**. an inline clause therefore produces cascading
--    deletes on one engine and orphaned rows on the other, from the same migration file.

-- the bytes, addressed by content. keyed by digest rather than by an id, which is what makes
-- republishing identical bytes free and lets two machines agree on the artifact for one source tree.
CREATE TABLE IF NOT EXISTS function_artifacts (
    digest VARCHAR(128) PRIMARY KEY,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    media_type VARCHAR(128) NOT NULL DEFAULT 'application/zip',
    created_at BIGINT NOT NULL
);

-- the named unit that owns versions and aliases. carries no code itself, so grants and aliases have
-- something stable to point at while versions come and go.
CREATE TABLE IF NOT EXISTS function_packages (
    id BINARY(16) PRIMARY KEY,
    org_id BINARY(16) NULL,
    namespace VARCHAR(255) NULL,
    name VARCHAR(255) NOT NULL,
    identity_key VARCHAR(512) NOT NULL,
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
CREATE UNIQUE INDEX idx_function_packages_identity
    ON function_packages(identity_key);
CREATE INDEX idx_function_packages_org ON function_packages(org_id);

-- one immutable release. never updated after insert: a workflow revision that pinned this version
-- must keep meaning what it meant.
CREATE TABLE IF NOT EXISTS function_versions (
    id BINARY(16) PRIMARY KEY,
    package_id BINARY(16) NOT NULL,
    version BIGINT NOT NULL,
    artifact_digest VARCHAR(128) NOT NULL,
    manifest LONGTEXT NOT NULL,
    runtime LONGTEXT NOT NULL,
    published_by BINARY(16) NULL,
    created_at BIGINT NOT NULL,
    CONSTRAINT fk_function_versions_package FOREIGN KEY (package_id)
        REFERENCES function_packages(id) ON DELETE CASCADE,
    CONSTRAINT fk_function_versions_artifact FOREIGN KEY (artifact_digest)
        REFERENCES function_artifacts(digest)
);

-- the version number is how a binding names its code, so a duplicate must be impossible rather than
-- unlikely: two racing publishes lose one insert instead of forking what "version 3" means.
CREATE UNIQUE INDEX idx_function_versions_seq
    ON function_versions(package_id, version);
CREATE INDEX idx_function_versions_artifact
    ON function_versions(artifact_digest);

-- one callable entry point with a typed signature.
CREATE TABLE IF NOT EXISTS function_exports (
    id BINARY(16) PRIMARY KEY,
    version_id BINARY(16) NOT NULL,
    name VARCHAR(255) NOT NULL,
    handler VARCHAR(512) NOT NULL,
    description TEXT NULL,
    input LONGTEXT NOT NULL,
    output LONGTEXT NOT NULL,
    limits LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    CONSTRAINT fk_function_exports_version FOREIGN KEY (version_id)
        REFERENCES function_versions(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_function_exports_name
    ON function_exports(version_id, name);

-- the one movable pointer at a version.
CREATE TABLE IF NOT EXISTS function_aliases (
    id BINARY(16) PRIMARY KEY,
    package_id BINARY(16) NOT NULL,
    name VARCHAR(255) NOT NULL,
    version_id BINARY(16) NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_function_aliases_package FOREIGN KEY (package_id)
        REFERENCES function_packages(id) ON DELETE CASCADE,
    CONSTRAINT fk_function_aliases_version FOREIGN KEY (version_id)
        REFERENCES function_versions(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_function_aliases_name
    ON function_aliases(package_id, name);
CREATE INDEX idx_function_aliases_version
    ON function_aliases(version_id);

-- the hidden workflow generated per export so a direct http invocation runs the same reducer path a
-- workflow call does, rather than a second execution engine.
CREATE TABLE IF NOT EXISTS function_adapter_workflows (
    id BINARY(16) PRIMARY KEY,
    export_id BINARY(16) NOT NULL,
    workflow_id BINARY(16) NOT NULL,
    created_at BIGINT NOT NULL,
    CONSTRAINT fk_function_adapter_export FOREIGN KEY (export_id)
        REFERENCES function_exports(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_function_adapter_export
    ON function_adapter_workflows(export_id);
CREATE INDEX idx_function_adapter_workflow
    ON function_adapter_workflows(workflow_id);
