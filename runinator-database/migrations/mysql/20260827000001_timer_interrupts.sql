-- Durable schedules for workflow-owned repeating interrupt handlers. A row remains scoped to the
-- run rather than a continuation because a timer must survive forks and handler completion.
CREATE TABLE IF NOT EXISTS workflow_timer_interrupts (
    workflow_run_id BINARY(16) NOT NULL,
    timer_id VARCHAR(255) NOT NULL,
    interval_seconds BIGINT NOT NULL,
    next_due_at BIGINT NOT NULL,
    PRIMARY KEY (workflow_run_id, timer_id),
    CONSTRAINT chk_workflow_timer_interrupt_interval CHECK (interval_seconds > 0),
    CONSTRAINT fk_workflow_timer_interrupts_run FOREIGN KEY (workflow_run_id)
        REFERENCES workflow_runs(id) ON DELETE CASCADE
);

CREATE INDEX idx_workflow_timer_interrupts_due
    ON workflow_timer_interrupts(next_due_at, workflow_run_id, timer_id);
