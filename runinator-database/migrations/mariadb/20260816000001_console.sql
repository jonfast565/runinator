-- the wdl console: a notebook of cells evaluated against a shared, persisted scope.
--
-- the same three mysql shapes as the functions migration, all forced rather than chosen: indexed
-- strings are VARCHAR (mysql cannot index TEXT without a prefix), and foreign keys are table-level
-- CONSTRAINTs because **mariadb creates a real foreign key from an inline `REFERENCES` clause while
-- mysql 8 parses and silently discards it** — the same file would otherwise cascade on one engine
-- and orphan rows on the other. indexes also drop `IF NOT EXISTS`, which mysql does not accept on
-- CREATE INDEX at all.
--
-- the scope lives in the database rather than in a replica's memory on purpose. a console session
-- outlives any one request, and a scope accumulated in process would give different answers
-- depending on which ws replica served the cell — which is the kind of bug nobody reproduces.

-- one notebook.
CREATE TABLE IF NOT EXISTS console_sessions (
    id BINARY(16) PRIMARY KEY,
    org_id BINARY(16) NULL,
    name VARCHAR(255) NOT NULL,
    created_by BINARY(16) NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX idx_console_sessions_org ON console_sessions(org_id);
CREATE INDEX idx_console_sessions_created_by ON console_sessions(created_by);

-- one cell. `position` orders them; `kind` records what the classifier decided, so a reader can see
-- why a cell did or did not start a run without re-classifying its source.
CREATE TABLE IF NOT EXISTS console_cells (
    id BINARY(16) PRIMARY KEY,
    session_id BINARY(16) NOT NULL,
    position BIGINT NOT NULL,
    label TEXT NULL,
    source TEXT NOT NULL,
    kind TEXT NULL,
    status TEXT NOT NULL,
    -- the value the cell produced, and the error if it failed. both nullable: a cell that has never
    -- been run has neither.
    result TEXT NULL,
    error TEXT NULL,
    -- set only for a cell that became a scratch workflow, which is what links it to its run.
    workflow_run_id BINARY(16) NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_console_cells_session FOREIGN KEY (session_id)
        REFERENCES console_sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_console_cells_session ON console_cells(session_id, position);
CREATE INDEX idx_console_cells_run ON console_cells(workflow_run_id);

-- the session's scope: what `cells.<name>` resolves to.
--
-- stored separately from the cell rather than read off it, because a binding survives its cell being
-- edited or re-run and because resolving a scope should be one query rather than a scan of every
-- cell's result.
CREATE TABLE IF NOT EXISTS console_bindings (
    id BINARY(16) PRIMARY KEY,
    session_id BINARY(16) NOT NULL,
    name VARCHAR(255) NOT NULL,
    cell_id BINARY(16) NULL,
    value TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_console_bindings_session FOREIGN KEY (session_id)
        REFERENCES console_sessions(id) ON DELETE CASCADE
);

-- one binding per name per session: re-running a cell replaces what its name resolves to rather
-- than adding a second row the scope builder would have to pick between.
CREATE UNIQUE INDEX idx_console_bindings_name
    ON console_bindings(session_id, name);
CREATE INDEX idx_console_bindings_cell ON console_bindings(cell_id);
