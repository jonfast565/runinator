INSERT INTO
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
        'Test: Hello World',
        '*/1 * * * *',
        'Console',
        'run_console',
        'echo ''Hello World!''',
        1000,
        1737008700,
        1
    );

INSERT INTO
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
        'AWS Login',
        '0 0,9,12,15,18,21 * * *',
        'Console',
        'run_console',
        'aws sso login',
        100000,
        1737018000,
        1
    );