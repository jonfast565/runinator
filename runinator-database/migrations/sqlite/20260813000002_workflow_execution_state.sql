CREATE TABLE IF NOT EXISTS workflow_run_execution_states (
    workflow_run_id BLOB PRIMARY KEY REFERENCES workflow_runs(id) ON DELETE CASCADE,
    watch_fired BOOLEAN NOT NULL DEFAULT FALSE,
    run_metadata_json TEXT NULL,
    extra_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS workflow_run_frames (
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    frame_kind TEXT NOT NULL,
    frame_json TEXT NOT NULL,
    PRIMARY KEY (workflow_run_id, frame_kind)
);

CREATE TABLE IF NOT EXISTS workflow_run_cursors (
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    cursor_id BLOB NOT NULL,
    position INTEGER NOT NULL,
    node_id TEXT NOT NULL,
    forked_by TEXT NULL,
    suspended_by BLOB NULL,
    suspended_seconds INTEGER NOT NULL DEFAULT 0,
    last_output_json TEXT NULL,
    PRIMARY KEY (workflow_run_id, cursor_id),
    UNIQUE (workflow_run_id, position)
);
CREATE INDEX idx_workflow_run_cursors_node ON workflow_run_cursors(workflow_run_id, node_id);

CREATE TABLE IF NOT EXISTS workflow_cursor_frames (
    workflow_run_id BLOB NOT NULL,
    cursor_id BLOB NOT NULL,
    frame_kind TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    node_id TEXT NULL,
    frame_json TEXT NOT NULL,
    PRIMARY KEY (workflow_run_id, cursor_id, frame_kind, position),
    FOREIGN KEY (workflow_run_id, cursor_id)
        REFERENCES workflow_run_cursors(workflow_run_id, cursor_id) ON DELETE CASCADE
);
CREATE INDEX idx_workflow_cursor_frames_kind
    ON workflow_cursor_frames(workflow_run_id, frame_kind, node_id);

CREATE TABLE IF NOT EXISTS workflow_run_event_sources (
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    pending_event_json TEXT NULL,
    PRIMARY KEY (workflow_run_id, node_id)
);

CREATE TABLE IF NOT EXISTS workflow_run_pending_interrupts (
    workflow_run_id BLOB NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    interrupt_id BLOB NOT NULL,
    source TEXT NOT NULL,
    payload_json TEXT NULL,
    cursor_id BLOB NULL,
    requested_at INTEGER NOT NULL,
    PRIMARY KEY (workflow_run_id, interrupt_id)
);
CREATE INDEX idx_workflow_run_pending_interrupts_target
    ON workflow_run_pending_interrupts(workflow_run_id, cursor_id, requested_at);
