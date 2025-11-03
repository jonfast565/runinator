use std::{fs, path::PathBuf, str::FromStr};

use chrono::{DateTime, Duration, Utc};
use futures_util::stream::StreamExt;
use log::{debug, info};
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    errors::SendableError,
};
use sqlx::{
    ConnectOptions, Executor, PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::{interfaces::DatabaseImpl, mappers};

const POSTGRES_TABLE_INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    cron_schedule TEXT NOT NULL,
    action_name TEXT NOT NULL,
    action_function TEXT NOT NULL,
    action_configuration TEXT NOT NULL,
    timeout BIGINT NOT NULL,
    next_execution BIGINT NULL,
    enabled BOOLEAN NOT NULL,
    immediate BOOLEAN NOT NULL,
    blackout_start BIGINT NULL,
    blackout_end BIGINT NULL
);

CREATE TABLE IF NOT EXISTS task_runs (
    id BIGSERIAL PRIMARY KEY,
    task_id BIGINT NOT NULL REFERENCES scheduled_tasks(id),
    start_time BIGINT NOT NULL,
    duration_ms BIGINT NOT NULL
);

COMMIT;
"#;

pub struct PostgresDb {
    pub pool: PgPool,
}

impl PostgresDb {
    pub async fn new(connection_str: &str) -> Result<Self, SendableError> {
        let mut options = PgConnectOptions::from_str(connection_str)?;
        options.log_statements(log::LevelFilter::Info);
        options.log_slow_statements(
            log::LevelFilter::Warn,
            Duration::seconds(1).to_std().unwrap(),
        );

        let pool = PgPoolOptions::new().connect_with(options).await?;
        Ok(Self { pool })
    }

    async fn execute_script(&self, script: &str) -> Result<(), SendableError> {
        let sql = script.trim();
        if sql.is_empty() {
            return Ok(());
        }

        for statement in sql.split(';') {
            let stmt = statement.trim();
            if stmt.is_empty() {
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

impl DatabaseImpl for PostgresDb {
    async fn upsert_task(&self, task: &ScheduledTask) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query(
                    "INSERT INTO scheduled_tasks (
                        id,
                        name,
                        cron_schedule,
                        action_name,
                        action_function,
                        action_configuration,
                        timeout,
                        next_execution,
                        enabled,
                        immediate,
                        blackout_start,
                        blackout_end
                    ) VALUES (
                        $1,
                        $2,
                        $3,
                        $4,
                        $5,
                        $6,
                        $7,
                        COALESCE($8, CAST(EXTRACT(EPOCH FROM NOW()) AS BIGINT)),
                        $9,
                        $10,
                        $11,
                        $12
                    )
                    ON CONFLICT (id) DO UPDATE SET
                        name = EXCLUDED.name,
                        cron_schedule = EXCLUDED.cron_schedule,
                        action_name = EXCLUDED.action_name,
                        action_function = EXCLUDED.action_function,
                        action_configuration = EXCLUDED.action_configuration,
                        timeout = EXCLUDED.timeout,
                        next_execution = EXCLUDED.next_execution,
                        enabled = EXCLUDED.enabled,
                        immediate = EXCLUDED.immediate,
                        blackout_start = EXCLUDED.blackout_start,
                        blackout_end = EXCLUDED.blackout_end",
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
                .bind(task.blackout_end.map(|dt| dt.timestamp())),
            )
            .await?;
        Ok(())
    }

    async fn delete_task(&self, task_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(sqlx::query("DELETE FROM scheduled_tasks WHERE id = $1").bind(task_id))
            .await?;
        Ok(())
    }

    async fn fetch_all_tasks(&self) -> Result<Vec<ScheduledTask>, SendableError> {
        let rows = sqlx::query(
            "SELECT id,
                    name,
                    cron_schedule,
                    action_name,
                    action_function,
                    action_configuration,
                    timeout,
                    next_execution,
                    enabled,
                    immediate,
                    blackout_start,
                    blackout_end
             FROM scheduled_tasks",
        )
        .fetch_all(&self.pool)
        .await?;

        let result = rows
            .into_iter()
            .map(|row| mappers::postgres_row_to_scheduled_task(&row))
            .collect();
        Ok(result)
    }

    async fn fetch_task_runs(&self, start: i64, end: i64) -> Result<Vec<TaskRun>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, task_id, start_time, duration_ms
             FROM task_runs
             WHERE start_time >= $1 AND start_time <= $2",
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
                sqlx::query(
                    "UPDATE scheduled_tasks
                     SET next_execution = $1
                     WHERE id = $2",
                )
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
                    "INSERT INTO task_runs (task_id, start_time, duration_ms)
                     VALUES ($1, $2, $3)",
                )
                .bind(task_id)
                .bind(start_time.timestamp())
                .bind(duration_ms),
            )
            .await?;
        Ok(())
    }

    async fn run_init_scripts(&self, paths: &Vec<String>) -> Result<(), SendableError> {
        info!("Running embedded Postgres table initialization script");
        self.execute_script(POSTGRES_TABLE_INIT_SQL).await?;
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
                sqlx::query(
                    "UPDATE scheduled_tasks
                     SET immediate = TRUE
                     WHERE id = $1",
                )
                .bind(task_id),
            )
            .await?;
        Ok(())
    }

    async fn clear_immediate_run(&self, task_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query(
                    "UPDATE scheduled_tasks
                     SET immediate = FALSE
                     WHERE id = $1",
                )
                .bind(task_id),
            )
            .await?;
        Ok(())
    }
}
