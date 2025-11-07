use std::{fs, path::PathBuf};

use chrono::{DateTime, Duration, Utc};
use futures_util::stream::StreamExt;
use log::{debug, info};
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    errors::SendableError,
};
use sqlx::{ConnectOptions, Executor, Row, SqlitePool, sqlite::SqliteConnectOptions};

use crate::{interfaces::DatabaseImpl, mappers};

const SQLITE_TABLE_INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    cron_schedule TEXT NOT NULL,
    action_name TEXT NOT NULL,
    action_function TEXT NOT NULL,
    action_configuration BLOB NOT NULL,
    timeout INTEGER NOT NULL,
    next_execution INTEGER NULL,
    enabled BOOL NOT NULL,
    immediate BOOL NOT NULL,
    blackout_start INTEGER NULL,
    blackout_end INTEGER NULL
);

CREATE TABLE IF NOT EXISTS task_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES scheduled_tasks(id),
    start_time INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL
);
"#;

pub struct SqliteDb {
    pub pool: SqlitePool,
}

impl SqliteDb {
    pub async fn new(filename: &str) -> Result<Self, SendableError> {
        let mut options = SqliteConnectOptions::new()
            .filename(filename)
            .create_if_missing(true);
        let options_with_logs = options
            .log_statements(log::LevelFilter::Debug)
            .log_slow_statements(
                log::LevelFilter::Warn,
                Duration::seconds(1).to_std().unwrap(),
            );
        let unmutable_options = options_with_logs.clone();
        let connection = SqlitePool::connect_with(unmutable_options).await?;
        let result = SqliteDb { pool: connection };
        Ok(result)
    }

    async fn execute_script(&self, script: &str) -> Result<(), SendableError> {
        let sql = script.trim();
        if sql.is_empty() {
            debug!("No SQL to execute");
            return Ok(());
        }

        for statement in sql.split(';') {
            let stmt = statement.trim();
            if stmt.is_empty() {
                debug!("Skipping empty statement");
                continue;
            }

            let mut stream = self.pool.execute_many(sqlx::query(stmt));
            while let Some(result) = stream.next().await {
                let query_result = result?;
                debug!(
                    "Init scripts: {} row(s) affected",
                    query_result.rows_affected()
                );
            }
        }

        Ok(())
    }
}

impl DatabaseImpl for SqliteDb {
    async fn upsert_task(&self, task: &ScheduledTask) -> Result<(), SendableError> {
        self.pool.execute(sqlx::query(
            "INSERT INTO scheduled_tasks (id, name, cron_schedule, action_name, action_function, action_configuration, timeout, next_execution, enabled, immediate, blackout_start, blackout_end)
             VALUES (?, ?, ?, ?, ?, ?, ?, COALESCE(?, unixepoch('now')), ?, COALESCE(?, 0), ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                cron_schedule = excluded.cron_schedule,
                action_name = excluded.action_name,
                action_configuration = excluded.action_configuration,
                timeout = excluded.timeout,
                next_execution = excluded.next_execution,
                enabled = excluded.enabled,
                immediate = excluded.immediate,
                blackout_start = excluded.blackout_start,
                blackout_end = excluded.blackout_end",
        )
        .bind(task.id)
        .bind(&task.name)
        .bind(&task.cron_schedule)
        .bind(&task.action_name)
        .bind(&task.action_function)
        .bind(&task.action_configuration)
        .bind(task.timeout)
        .bind(task.next_execution.map(|dt| dt.timestamp()))
        .bind(task.enabled)
        .bind(task.immediate)
        .bind(task.blackout_start.map(|dt| dt.timestamp()))
        .bind(task.blackout_end.map(|dt| dt.timestamp())))
        .await?;
        Ok(())
    }

    async fn delete_task(&self, task_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(sqlx::query("DELETE FROM scheduled_tasks WHERE id = ?").bind(task_id))
            .await?;
        Ok(())
    }

    async fn fetch_all_tasks(&self) -> Result<Vec<ScheduledTask>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, name, cron_schedule, action_name, action_function, action_configuration, timeout, next_execution, enabled, immediate, blackout_start, blackout_end 
            FROM scheduled_tasks",
        )
        .fetch_all(&self.pool)
        .await?;

        let result = rows
            .into_iter()
            .map(|row| mappers::sqlite_row_to_scheduled_task(&row))
            .collect();
        Ok(result)
    }

    async fn fetch_task_by_id(&self, task_id: i64) -> Result<Option<ScheduledTask>, SendableError> {
        let row = sqlx::query(
            "SELECT id, name, cron_schedule, action_name, action_function, action_configuration, timeout, next_execution, enabled, immediate, blackout_start, blackout_end
            FROM scheduled_tasks WHERE id = ?",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| mappers::sqlite_row_to_scheduled_task(&row)))
    }

    async fn fetch_task_runs(&self, start: i64, end: i64) -> Result<Vec<TaskRun>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, task_id, start_time, duration_ms FROM task_runs WHERE start_time >= ? AND start_time <= ?",
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await?;

        let result = rows
            .into_iter()
            .map(|row| TaskRun {
                id: row.get("id"),
                task_id: row.get("task_id"),
                start_time: row.get("start_time"),
                duration_ms: row.get("duration_ms"),
            })
            .collect();
        Ok(result)
    }

    async fn update_task_next_execution(&self, task: &ScheduledTask) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query("UPDATE scheduled_tasks SET next_execution = ? WHERE id = ?")
                    .bind(task.next_execution.map(|dt| dt.timestamp()))
                    .bind(task.id),
            )
            .await?;
        Ok(())
    }

    async fn log_task_run(
        &self,
        task_id: i64,
        start_time: DateTime<Utc>,
        duration_ms: i64,
    ) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query(
                    "INSERT INTO task_runs (task_id, start_time, duration_ms) VALUES (?, ?, ?)",
                )
                .bind(task_id)
                .bind(start_time.timestamp())
                .bind(duration_ms),
            )
            .await?;
        Ok(())
    }

    async fn run_init_scripts(&self, paths: &Vec<String>) -> Result<(), SendableError> {
        info!("Running embedded SQLite table initialization script");
        self.execute_script(SQLITE_TABLE_INIT_SQL).await?;
        for path in paths.iter() {
            let path_info = PathBuf::from(path);
            if path_info.extension().and_then(|ext| ext.to_str()) == Some("sql") {
                info!("Running {}", path_info.to_str().unwrap());
                let script = fs::read_to_string(path_info.as_path())?;
                self.execute_script(&script).await?;
            }
        }

        Ok(())
    }

    async fn request_immediate_run(&self, task_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query("UPDATE scheduled_tasks SET immediate = 1 WHERE id = ?").bind(task_id),
            )
            .await?;
        Ok(())
    }

    async fn clear_immediate_run(&self, task_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query("UPDATE scheduled_tasks SET immediate = 0 WHERE id = ?").bind(task_id),
            )
            .await?;
        Ok(())
    }
}
