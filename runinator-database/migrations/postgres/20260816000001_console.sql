-- the rexrap console: a notebook of cells evaluated against a shared, persisted scope.
--
-- the scope lives in the database rather than in a replica's memory on purpose. a console session
-- outlives any one request, and a scope accumulated in process would give different answers
-- depending on which ws replica served the cell — which is the kind of bug nobody reproduces.

-- one notebook.
CREATE TABLE IF NOT EXISTS console_sessions (
    id UUID PRIMARY KEY,
    org_id UUID NULL,
    name TEXT NOT NULL,
    created_by UUID NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_console_sessions_org ON console_sessions(org_id);
CREATE INDEX IF NOT EXISTS idx_console_sessions_created_by ON console_sessions(created_by);

-- one cell. `position` orders them; `kind` records what the classifier decided, so a reader can see
-- why a cell did or did not start a run without re-classifying its source.
CREATE TABLE IF NOT EXISTS console_cells (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES console_sessions(id) ON DELETE CASCADE,
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
    workflow_run_id UUID NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_console_cells_session ON console_cells(session_id, position);
CREATE INDEX IF NOT EXISTS idx_console_cells_run ON console_cells(workflow_run_id);

-- the session's scope: what `cells.<name>` resolves to.
--
-- stored separately from the cell rather than read off it, because a binding survives its cell being
-- edited or re-run and because resolving a scope should be one query rather than a scan of every
-- cell's result.
CREATE TABLE IF NOT EXISTS console_bindings (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES console_sessions(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    cell_id UUID NULL,
    value TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- one binding per name per session: re-running a cell replaces what its name resolves to rather
-- than adding a second row the scope builder would have to pick between.
CREATE UNIQUE INDEX IF NOT EXISTS idx_console_bindings_name
    ON console_bindings(session_id, name);
CREATE INDEX IF NOT EXISTS idx_console_bindings_cell ON console_bindings(cell_id);
