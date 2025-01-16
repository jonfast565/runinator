CREATE TABLE
    IF NOT EXISTS scheduled_tasks (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
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
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id INTEGER NOT NULL REFERENCES scheduled_tasks(id),
        start_time INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL
    );