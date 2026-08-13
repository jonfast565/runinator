CREATE TABLE IF NOT EXISTS workflow_run_execution_states (
    workflow_run_id BINARY(16) PRIMARY KEY REFERENCES workflow_runs(id) ON DELETE CASCADE,
    watch_fired BOOLEAN NOT NULL DEFAULT FALSE,
    run_metadata_json TEXT NULL,
    extra_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_run_frames (
    workflow_run_id BINARY(16) NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    frame_kind VARCHAR(64) NOT NULL,
    frame_json TEXT NOT NULL,
    PRIMARY KEY (workflow_run_id, frame_kind)
);

CREATE TABLE IF NOT EXISTS workflow_run_cursors (
    workflow_run_id BINARY(16) NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    cursor_id BINARY(16) NOT NULL,
    position BIGINT NOT NULL,
    node_id VARCHAR(512) NOT NULL,
    forked_by TEXT NULL,
    suspended_by BINARY(16) NULL,
    suspended_seconds BIGINT NOT NULL DEFAULT 0,
    last_output_json TEXT NULL,
    PRIMARY KEY (workflow_run_id, cursor_id),
    UNIQUE KEY uq_workflow_run_cursors_position (workflow_run_id, position),
    KEY idx_workflow_run_cursors_node (workflow_run_id, node_id)
);

CREATE TABLE IF NOT EXISTS workflow_cursor_frames (
    workflow_run_id BINARY(16) NOT NULL,
    cursor_id BINARY(16) NOT NULL,
    frame_kind VARCHAR(64) NOT NULL,
    position BIGINT NOT NULL DEFAULT 0,
    node_id VARCHAR(512) NULL,
    frame_json TEXT NOT NULL,
    PRIMARY KEY (workflow_run_id, cursor_id, frame_kind, position),
    KEY idx_workflow_cursor_frames_kind (workflow_run_id, frame_kind, node_id),
    FOREIGN KEY (workflow_run_id, cursor_id)
        REFERENCES workflow_run_cursors(workflow_run_id, cursor_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS workflow_run_event_sources (
    workflow_run_id BINARY(16) NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    node_id VARCHAR(512) NOT NULL,
    pending_event_json TEXT NULL,
    PRIMARY KEY (workflow_run_id, node_id)
);

CREATE TABLE IF NOT EXISTS workflow_run_pending_interrupts (
    workflow_run_id BINARY(16) NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    interrupt_id BINARY(16) NOT NULL,
    source VARCHAR(64) NOT NULL,
    payload_json TEXT NULL,
    cursor_id BINARY(16) NULL,
    requested_at BIGINT NOT NULL,
    PRIMARY KEY (workflow_run_id, interrupt_id),
    KEY idx_workflow_run_pending_interrupts_target (workflow_run_id, cursor_id, requested_at)
);
