use chrono::{DateTime, Duration, Utc};
use runinator_models::core::{ScheduledTask, TaskRun};
use sqlx::{sqlite::SqliteConnectOptions, ConnectOptions, Executor, Row, SqlitePool};

use crate::interfaces::DatabaseImpl;

pub struct SqliteDb {
    pub pool: SqlitePool,
}

impl SqliteDb {
    pub async fn new(filename: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut options = SqliteConnectOptions::new()
        .filename(filename)
        .create_if_missing(true);
        let options_with_logs = options
            .log_statements(log::LevelFilter::Debug)
            .log_slow_statements(log::LevelFilter::Warn, Duration::seconds(1).to_std().unwrap());
        let unmutable_options = options_with_logs.clone();
        let connection = SqlitePool::connect_with(unmutable_options).await?;
        let result = SqliteDb { pool: connection };
        Ok(result)
    }
}

impl DatabaseImpl for SqliteDb {
    async fn create_scheduled_tasks_table(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.pool
            .execute(
                "CREATE TABLE IF NOT EXISTS scheduled_tasks (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            cron_schedule TEXT NOT NULL,
            action_name TEXT NOT NULL,
            action_configuration BLOB NOT NULL,
            timeout INTEGER NOT NULL,
            next_execution INTEGER
        )",
            )
            .await?;
        Ok(())
    }

    async fn create_task_runs_table(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.pool
            .execute(
                "CREATE TABLE IF NOT EXISTS task_runs (
                id INTEGER PRIMARY KEY,
                task_name TEXT NOT NULL,
                start_time INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL
            )",
            )
            .await?;
        Ok(())
    }

    async fn upsert_task(&self, task: &ScheduledTask) -> Result<(), Box<dyn std::error::Error>> {
        self.pool.execute(sqlx::query(
            "INSERT INTO scheduled_tasks (id, name, cron_schedule, action_name, action_configuration, timeout, next_execution)
             VALUES (?, ?, ?, ?, ?, ?, ?)
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
        .bind(&task.action_configuration)
        .bind(task.timeout)
        .bind(task.next_execution.map(|dt| dt.timestamp())))
        .await?;
        Ok(())
    }

    async fn delete_task(&self, task_id: i64) -> Result<(), Box<dyn std::error::Error>> {
        self.pool
            .execute(sqlx::query("DELETE FROM scheduled_tasks WHERE id = ?").bind(task_id))
            .await?;
        Ok(())
    }

    async fn fetch_all_tasks(&self) -> Result<Vec<ScheduledTask>, Box<dyn std::error::Error>> {
        let rows = sqlx::query(
            "SELECT id, name, cron_schedule, action_name, action_configuration, timeout, next_execution FROM scheduled_tasks",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap();

        let result = rows
            .into_iter()
            .map(|row| ScheduledTask {
                id: row.get("id"),
                name: row.get("name"),
                cron_schedule: row.get("cron_schedule"),
                action_name: row.get("action_name"),
                action_configuration: row.get("action_configuration"),
                timeout: row.get("timeout"),
                next_execution: row
                    .get::<Option<i64>, _>("next_execution")
                    .map(|ts| DateTime::from_timestamp(ts, 0))
                    .unwrap(),
            })
            .collect();
        Ok(result)
    }

    async fn fetch_task_runs(
        &self,
        start: i64,
        end: i64,
    ) -> Result<Vec<TaskRun>, Box<dyn std::error::Error>> {
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
                task_name: row.get("task_name"),
                start_time: row.get("start_time"),
                duration_ms: row.get("duration_ms"),
            })
            .collect();
        Ok(result)
    }

    async fn update_task_next_execution(
        &self,
        task: &ScheduledTask,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
        task_name: &str,
        start_time: DateTime<Utc>,
        duration_ms: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.pool
            .execute(
                sqlx::query(
                    "INSERT INTO task_runs (task_name, start_time, duration_ms) VALUES (?, ?, ?)",
                )
                .bind(task_name)
                .bind(start_time.timestamp())
                .bind(duration_ms),
            )
            .await?;
        Ok(())
    }
}
