use chrono::{DateTime, Utc};
use runinator_models::core::ScheduledTask;
use sqlx::{Row, sqlite::SqliteRow};

pub fn row_to_scheduled_task(row: &SqliteRow) -> ScheduledTask {
    let next_execution = row
        .get::<Option<i64>, _>("next_execution")
        .map(|ts| DateTime::<Utc>::from_timestamp(ts, 0));
    let next_execution_part = match next_execution {
        Some(x) => x,
        None => None,
    };

    ScheduledTask {
        id: row.get::<Option<i64>, _>("id"),
        name: row.get::<String, _>("name"),
        cron_schedule: row.get::<String, _>("cron_schedule"),
        action_name: row.get::<String, _>("action_name"),
        action_function: row.get::<String, _>("action_function"),
        action_configuration: row.get::<String, _>("action_configuration"),
        timeout: row.get::<i64, _>("timeout"),
        next_execution: next_execution_part,
        enabled: row.get::<bool, _>("enabled"),
        immediate: row.get::<bool, _>("immediate"),
    }
}
