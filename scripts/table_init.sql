CREATE TABLE
    IF NOT EXISTS scheduled_tasks (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        cron_schedule TEXT NOT NULL,
        action_name TEXT NOT NULL,
        action_function TEXT NOT NULL,
        action_configuration BLOB NOT NULL,
        timeout INTEGER NOT NULL,
        next_execution INTEGER NULL,
        enabled BOOL NOT NULL
    );

CREATE TABLE
    IF NOT EXISTS task_runs (
        id INTEGER NOT NULL,
        task_id INTEGER NOT NULL,
        start_time INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL,
        PRIMARY KEY (id, task_id)
    );