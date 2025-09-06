use chrono::{DateTime, Utc};
use runinator_models::core::ScheduledTask;
use sqlx::{Row, sqlite::SqliteRow};

pub fn row_to_scheduled_task(row: &SqliteRow) -> ScheduledTask {
    let next_execution = row
        .get::<Option<i64>, _>("next_execution")
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

    let blackout_start = row
        .get::<Option<i64>, _>("blackout_start")
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

    let blackout_end = row
        .get::<Option<i64>, _>("blackout_end")
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

    ScheduledTask {
        id: row.get::<Option<i64>, _>("id"),
        name: row.get::<String, _>("name"),
        cron_schedule: row.get::<String, _>("cron_schedule"),
        action_name: row.get::<String, _>("action_name"),
        action_function: row.get::<String, _>("action_function"),
        action_configuration: row.get::<String, _>("action_configuration"),
        timeout: row.get::<i64, _>("timeout"),
        next_execution: next_execution,
        enabled: row.get::<bool, _>("enabled"),
        immediate: row.get::<bool, _>("immediate"),
        blackout_start: blackout_start,
        blackout_end: blackout_end,
    }
}
