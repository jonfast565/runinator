-- active top-level REXRAP definitions for one durable console session. A row is the latest
-- successful publisher for its `(session_id, name)`; the implementation replaces it atomically.
CREATE TABLE IF NOT EXISTS console_functions (
    id BLOB PRIMARY KEY,
    session_id BLOB NOT NULL REFERENCES console_sessions(id) ON DELETE CASCADE,
    cell_id BLOB NOT NULL REFERENCES console_cells(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    is_task BOOLEAN NOT NULL,
    source TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_console_functions_name
    ON console_functions(session_id, name);
CREATE INDEX IF NOT EXISTS idx_console_functions_cell ON console_functions(cell_id);
