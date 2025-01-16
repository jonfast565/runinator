use std::{fs, path::PathBuf};

use chrono::{DateTime, Duration, Utc};
use log::{debug, info};
use runinator_models::{core::{ScheduledTask, TaskRun}, errors::SendableError};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteRow},
    ConnectOptions, Executor, Row, SqlitePool,
};
use futures_util::stream::StreamExt;

use crate::interfaces::DatabaseImpl;

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
}

impl DatabaseImpl for SqliteDb {
    async fn upsert_task(&self, task: &ScheduledTask) -> Result<(), SendableError> {
        self.pool.execute(sqlx::query(
            "INSERT INTO scheduled_tasks (id, name, cron_schedule, action_name, action_function, action_configuration, timeout, next_execution, enabled)
             VALUES (?, ?, ?, ?, ?, ?, ?, COALESCE(next_execution, now()), ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                cron_schedule = excluded.cron_schedule,
                action_name = excluded.action_name,
                action_configuration = excluded.action_configuration,
                timeout = excluded.timeout,
                next_execution = excluded.next_execution",
        )
        .bind(task.id)
        .bind(&task.name)
        .bind(&task.cron_schedule)
        .bind(&task.action_name)
        .bind(&task.action_function)
        .bind(&task.action_configuration)
        .bind(task.timeout)
        .bind(task.next_execution.map(|dt| dt.timestamp()))
        .bind(task.enabled))
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
            "SELECT id, name, cron_schedule, action_name, action_function, action_configuration, timeout, next_execution, enabled FROM scheduled_tasks",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap();

        let result = rows
            .into_iter()
            .map(|row| row_to_scheduled_task(&row))
            .collect();
        Ok(result)
    }

    async fn fetch_task_runs(
        &self,
        start: i64,
        end: i64,
    ) -> Result<Vec<TaskRun>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, task_name, start_time, duration_ms FROM task_runs WHERE start_time >= ? AND start_time <= ?",
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .unwrap();

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

    async fn update_task_next_execution(
        &self,
        task: &ScheduledTask,
    ) -> Result<(), SendableError> {
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
    
    async fn run_init_scripts(
        &self,
        paths: &Vec<String>,
    ) -> Result<(), SendableError> {
        for path in paths.iter() {
            let path_info = PathBuf::from(path);
            if path_info.extension().and_then(|ext| ext.to_str()) == Some("sql") {
                info!("Running {}", path_info.to_str().unwrap());
                let script = fs::read_to_string(path_info.as_path())?;
                let mut stream = self.pool.execute_many(sqlx::query(&script));
                while let Some(result) = stream.next().await {
                    let query_result = result?;
                    debug!("Init scripts: {} row(s) affected", query_result.rows_affected());
                }
            }
        }

        Ok(())
    }
}

fn row_to_scheduled_task(row: &SqliteRow) -> ScheduledTask {
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
        enabled: row.get::<bool, _>("enabled")
    }
}
