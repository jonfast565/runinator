-- Durable schedules for workflow-owned repeating interrupt handlers. A row remains scoped to the
-- run rather than a continuation because a timer must survive forks and handler completion.
CREATE TABLE IF NOT EXISTS workflow_timer_interrupts (
    workflow_run_id UUID NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
    timer_id TEXT NOT NULL,
    interval_seconds BIGINT NOT NULL CHECK (interval_seconds > 0),
    next_due_at BIGINT NOT NULL,
    PRIMARY KEY (workflow_run_id, timer_id)
);

CREATE INDEX IF NOT EXISTS idx_workflow_timer_interrupts_due
    ON workflow_timer_interrupts(next_due_at, workflow_run_id, timer_id);
