#[derive(Clone, Copy)]
pub(crate) enum SqlDialect {
    Sqlite,
    Postgres,
}

fn bind(dialect: SqlDialect, index: usize) -> String {
    match dialect {
        SqlDialect::Sqlite => "?".to_string(),
        SqlDialect::Postgres => format!("${index}"),
    }
}

pub(crate) fn fetch_runs_by_status(dialect: SqlDialect) -> String {
    format!(
        "SELECT id, status, parameters, output_json, message, trigger, started_at, finished_at, created_at, workflow_run_id, workflow_node_id FROM runs WHERE status = {} ORDER BY id",
        bind(dialect, 1)
    )
}

pub(crate) fn update_run_status(dialect: SqlDialect) -> String {
    let b = |n| bind(dialect, n);
    format!(
        "UPDATE runs SET status = {s1}, output_json = COALESCE({s2}, output_json), message = COALESCE({s3}, message), started_at = CASE WHEN {s4} = 'running' AND started_at IS NULL THEN {s5} ELSE started_at END, finished_at = CASE WHEN {s6} THEN {s7} ELSE finished_at END WHERE id = {s8}",
        s1 = b(1),
        s2 = b(2),
        s3 = b(3),
        s4 = b(4),
        s5 = b(5),
        s6 = b(6),
        s7 = b(7),
        s8 = b(8),
    )
}

pub(crate) fn claim_workflow_runs_for_scheduler(dialect: SqlDialect, statuses: &str) -> String {
    match dialect {
        SqlDialect::Postgres => format!(
            "UPDATE workflow_runs SET scheduler_claimed_by = $1, scheduler_claimed_until = $2
             WHERE id IN (
                 SELECT id FROM workflow_runs
                 WHERE status IN ({statuses})
                   AND (scheduler_claimed_until IS NULL OR scheduler_claimed_until <= $3 OR scheduler_claimed_by = $1)
                 ORDER BY id
                 LIMIT $4
                 FOR UPDATE SKIP LOCKED
             )
             RETURNING id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name"
        ),
        SqlDialect::Sqlite => format!(
            "UPDATE workflow_runs SET scheduler_claimed_by = ?, scheduler_claimed_until = ?
             WHERE id IN (
                 SELECT id FROM workflow_runs
                 WHERE status IN ({statuses})
                   AND (scheduler_claimed_until IS NULL OR scheduler_claimed_until <= ? OR scheduler_claimed_by = ?)
                 ORDER BY id
                 LIMIT ?
             )
             RETURNING id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name"
        ),
    }
}
