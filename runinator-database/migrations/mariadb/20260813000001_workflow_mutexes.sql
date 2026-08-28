-- normalized, cursor-scoped workflow mutex ownership and durable fifo waiters.
CREATE TABLE IF NOT EXISTS workflow_mutexes (
    name VARCHAR(512) PRIMARY KEY,
    holder_run_id BINARY(16) NULL,
    holder_cursor_id BINARY(16) NULL,
    acquired_at BIGINT NULL,
    hold_deadline BIGINT NULL,
    overdue_at BIGINT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_workflow_mutex_holder FOREIGN KEY (holder_run_id)
        REFERENCES workflow_runs(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS workflow_mutex_waiters (
    workflow_node_run_id BINARY(16) PRIMARY KEY,
    name VARCHAR(512) NOT NULL,
    workflow_run_id BINARY(16) NOT NULL,
    cursor_id BINARY(16) NOT NULL,
    node_id VARCHAR(255) NOT NULL,
    enqueued_at BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_workflow_mutex_waiter_node_run FOREIGN KEY (workflow_node_run_id)
        REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    CONSTRAINT fk_workflow_mutex_waiter_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE,
    INDEX idx_workflow_mutex_waiters_fifo (name, enqueued_at, workflow_node_run_id),
    INDEX idx_workflow_mutex_waiters_run (workflow_run_id)
);

CREATE INDEX idx_workflow_mutexes_holder ON workflow_mutexes(holder_run_id);
