DELETE FROM task_runs;

INSERT OR IGNORE INTO
    scheduled_tasks (
        id,
        name,
        cron_schedule,
        action_name,
        action_function,
        action_configuration,
        timeout,
        next_execution,
        enabled
    )
VALUES
    (
        1,
        'Test: Hello World',
        '*/1 * * * *',
        'Console',
        'run_console',
        'echo ''Hello World!''',
        1000,
        1737008700,
        1
    );

INSERT OR IGNORE INTO
    scheduled_tasks (
        name,
        cron_schedule,
        action_name,
        action_function,
        action_configuration,
        timeout,
        next_execution,
        enabled
    )
VALUES
    (
        2,
        '0 0,9,12,15,18,21 * * *',
        'Console',
        'run_console',
        'aws sso login',
        100000,
        1737018000,
        1
    );