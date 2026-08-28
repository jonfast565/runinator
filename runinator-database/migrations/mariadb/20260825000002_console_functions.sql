-- active top-level REXRAP definitions for one durable console session. MySQL needs explicit
-- table-level foreign keys and indexed strings must be VARCHAR rather than TEXT.
CREATE TABLE IF NOT EXISTS console_functions (
    id BINARY(16) PRIMARY KEY,
    session_id BINARY(16) NOT NULL,
    cell_id BINARY(16) NOT NULL,
    name VARCHAR(255) NOT NULL,
    is_task BOOLEAN NOT NULL,
    source TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_console_functions_session FOREIGN KEY (session_id)
        REFERENCES console_sessions(id) ON DELETE CASCADE,
    CONSTRAINT fk_console_functions_cell FOREIGN KEY (cell_id)
        REFERENCES console_cells(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_console_functions_name ON console_functions(session_id, name);
CREATE INDEX idx_console_functions_cell ON console_functions(cell_id);
