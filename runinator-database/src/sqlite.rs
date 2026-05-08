use std::{fs, path::PathBuf};

use chrono::{DateTime, Duration, Utc};
use futures_util::stream::StreamExt;
use log::{debug, info};
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    errors::SendableError,
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStepRun},
};
use serde_json::Value;
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
    blackout_end INTEGER NULL,
    input_schema TEXT NOT NULL DEFAULT '{"type":"object","additionalProperties":true}',
    default_parameters TEXT NOT NULL DEFAULT '{}',
    output_schema TEXT NULL,
    mcp_enabled BOOL NOT NULL DEFAULT 0,
    metadata TEXT NOT NULL DEFAULT '{}',
    tags TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE IF NOT EXISTS task_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES scheduled_tasks(id),
    start_time INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES scheduled_tasks(id),
    status TEXT NOT NULL,
    parameters TEXT NOT NULL,
    output_json TEXT NULL,
    message TEXT NULL,
    trigger TEXT NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL,
    created_at INTEGER NOT NULL,
    workflow_run_id INTEGER NULL,
    workflow_step_id TEXT NULL
);

CREATE TABLE IF NOT EXISTS run_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL REFERENCES runs(id),
    sequence INTEGER NOT NULL,
    stream TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS run_artifacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL REFERENCES runs(id),
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    uri TEXT NOT NULL,
    metadata TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflows (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    version INTEGER NOT NULL,
    enabled BOOL NOT NULL,
    input_schema TEXT NOT NULL,
    definition TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workflow_id INTEGER NOT NULL REFERENCES workflows(id),
    status TEXT NOT NULL,
    parameters TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL,
    message TEXT NULL
);

CREATE TABLE IF NOT EXISTS workflow_step_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workflow_run_id INTEGER NOT NULL REFERENCES workflow_runs(id),
    step_id TEXT NOT NULL,
    task_run_id INTEGER NULL REFERENCES runs(id),
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    parameters TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    started_at INTEGER NULL,
    finished_at INTEGER NULL,
    message TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status);
CREATE INDEX IF NOT EXISTS idx_runs_task_id ON runs(task_id);
CREATE INDEX IF NOT EXISTS idx_run_chunks_run_sequence ON run_chunks(run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_status ON workflow_runs(status);
CREATE INDEX IF NOT EXISTS idx_workflow_step_runs_workflow_run ON workflow_step_runs(workflow_run_id);
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
            "INSERT INTO scheduled_tasks (id, name, cron_schedule, action_name, action_function, action_configuration, timeout, next_execution, enabled, immediate, blackout_start, blackout_end, input_schema, default_parameters, output_schema, mcp_enabled, metadata, tags)
             VALUES (?, ?, ?, ?, ?, ?, ?, COALESCE(?, unixepoch('now')), ?, COALESCE(?, 0), ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                cron_schedule = excluded.cron_schedule,
                action_name = excluded.action_name,
                action_function = excluded.action_function,
                action_configuration = excluded.action_configuration,
                timeout = excluded.timeout,
                next_execution = excluded.next_execution,
                enabled = excluded.enabled,
                immediate = excluded.immediate,
                blackout_start = excluded.blackout_start,
                blackout_end = excluded.blackout_end,
                input_schema = excluded.input_schema,
                default_parameters = excluded.default_parameters,
                output_schema = excluded.output_schema,
                mcp_enabled = excluded.mcp_enabled,
                metadata = excluded.metadata,
                tags = excluded.tags",
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
        .bind(task.blackout_end.map(|dt| dt.timestamp()))
        .bind(task.input_schema.to_string())
        .bind(task.default_parameters.to_string())
        .bind(task.output_schema.as_ref().map(|v| v.to_string()))
        .bind(task.mcp_enabled)
        .bind(task.metadata.to_string())
        .bind(serde_json::to_string(&task.tags)?))
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
            "SELECT id, name, cron_schedule, action_name, action_function, action_configuration, timeout, next_execution, enabled, immediate, blackout_start, blackout_end, input_schema, default_parameters, output_schema, mcp_enabled, metadata, tags 
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
            "SELECT id, name, cron_schedule, action_name, action_function, action_configuration, timeout, next_execution, enabled, immediate, blackout_start, blackout_end, input_schema, default_parameters, output_schema, mcp_enabled, metadata, tags
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

    async fn create_task_run(
        &self,
        task_id: i64,
        parameters: Value,
        trigger: String,
        workflow_run_id: Option<i64>,
        workflow_step_id: Option<String>,
    ) -> Result<RunSummary, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO runs (task_id, status, parameters, trigger, created_at, workflow_run_id, workflow_step_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             RETURNING id, task_id, status, parameters, output_json, message, trigger, started_at, finished_at, created_at, workflow_run_id, workflow_step_id",
        )
        .bind(task_id)
        .bind(RunStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(trigger)
        .bind(now)
        .bind(workflow_run_id)
        .bind(workflow_step_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_run_summary(&row))
    }

    async fn fetch_run(&self, run_id: i64) -> Result<Option<RunSummary>, SendableError> {
        let row = sqlx::query(
            "SELECT id, task_id, status, parameters, output_json, message, trigger, started_at, finished_at, created_at, workflow_run_id, workflow_step_id FROM runs WHERE id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_run_summary(&row)))
    }

    async fn fetch_runs_for_task(&self, task_id: i64) -> Result<Vec<RunSummary>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, task_id, status, parameters, output_json, message, trigger, started_at, finished_at, created_at, workflow_run_id, workflow_step_id FROM runs WHERE task_id = ? ORDER BY id DESC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_run_summary)
            .collect())
    }

    async fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<RunSummary>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, task_id, status, parameters, output_json, message, trigger, started_at, finished_at, created_at, workflow_run_id, workflow_step_id FROM runs WHERE status = ? ORDER BY id",
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_run_summary)
            .collect())
    }

    async fn update_run_status(
        &self,
        run_id: i64,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = matches!(
            status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
        );
        self.pool.execute(sqlx::query(
            "UPDATE runs SET status = ?, output_json = COALESCE(?, output_json), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(output_json.map(|v| v.to_string()))
        .bind(message)
        .bind(status.as_str())
        .bind(now)
        .bind(terminal)
        .bind(now)
        .bind(run_id))
        .await?;
        Ok(())
    }

    async fn append_run_chunk(
        &self,
        run_id: i64,
        chunk: &NewRunChunk,
    ) -> Result<RunChunk, SendableError> {
        let sequence: i64 = sqlx::query("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM run_chunks WHERE run_id = ?")
            .bind(run_id)
            .fetch_one(&self.pool)
            .await?
            .get("next_sequence");
        let row = sqlx::query(
            "INSERT INTO run_chunks (run_id, sequence, stream, content, created_at)
             VALUES (?, ?, ?, ?, ?)
             RETURNING id, run_id, sequence, stream, content, created_at",
        )
        .bind(run_id)
        .bind(sequence)
        .bind(&chunk.stream)
        .bind(&chunk.content)
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_run_chunk(&row))
    }

    async fn fetch_run_chunks(
        &self,
        run_id: i64,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<RunChunk>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, run_id, sequence, stream, content, created_at FROM run_chunks WHERE run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT ?",
        )
        .bind(run_id)
        .bind(cursor.unwrap_or(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(mappers::sqlite_row_to_run_chunk).collect())
    }

    async fn add_run_artifact(
        &self,
        run_id: i64,
        artifact: &NewRunArtifact,
    ) -> Result<RunArtifact, SendableError> {
        let row = sqlx::query(
            "INSERT INTO run_artifacts (run_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             RETURNING id, run_id, name, mime_type, size_bytes, uri, metadata, created_at",
        )
        .bind(run_id)
        .bind(&artifact.name)
        .bind(&artifact.mime_type)
        .bind(artifact.size_bytes)
        .bind(&artifact.uri)
        .bind(artifact.metadata.to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_run_artifact(&row))
    }

    async fn fetch_run_artifacts(&self, run_id: i64) -> Result<Vec<RunArtifact>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts WHERE run_id = ? ORDER BY id ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_run_artifact)
            .collect())
    }

    async fn fetch_artifact(&self, artifact_id: i64) -> Result<Option<RunArtifact>, SendableError> {
        let row = sqlx::query(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts WHERE id = ?",
        )
        .bind(artifact_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_run_artifact(&row)))
    }

    async fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO workflows (id, name, version, enabled, input_schema, definition, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, version = excluded.version, enabled = excluded.enabled, input_schema = excluded.input_schema, definition = excluded.definition, updated_at = excluded.updated_at
             RETURNING id, name, version, enabled, input_schema, definition, created_at, updated_at",
        )
        .bind(workflow.id)
        .bind(&workflow.name)
        .bind(workflow.version)
        .bind(workflow.enabled)
        .bind(workflow.input_schema.to_string())
        .bind(workflow.definition.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_workflow(&row))
    }

    async fn fetch_workflows(&self) -> Result<Vec<WorkflowDefinition>, SendableError> {
        let rows = sqlx::query("SELECT id, name, version, enabled, input_schema, definition, created_at, updated_at FROM workflows ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(mappers::sqlite_row_to_workflow).collect())
    }

    async fn fetch_workflow(
        &self,
        workflow_id: i64,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        let row = sqlx::query("SELECT id, name, version, enabled, input_schema, definition, created_at, updated_at FROM workflows WHERE id = ?")
            .bind(workflow_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_workflow(&row)))
    }

    async fn delete_workflow(&self, workflow_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(sqlx::query("DELETE FROM workflows WHERE id = ?").bind(workflow_id))
            .await?;
        Ok(())
    }

    async fn create_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
    ) -> Result<WorkflowRun, SendableError> {
        let row = sqlx::query(
            "INSERT INTO workflow_runs (workflow_id, status, parameters, created_at)
             VALUES (?, ?, ?, ?)
             RETURNING id, workflow_id, status, parameters, created_at, started_at, finished_at, message",
        )
        .bind(workflow_id)
        .bind(RunStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_workflow_run(&row))
    }

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<Option<WorkflowRun>, SendableError> {
        let row = sqlx::query("SELECT id, workflow_id, status, parameters, created_at, started_at, finished_at, message FROM workflow_runs WHERE id = ?")
            .bind(workflow_run_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_workflow_run(&row)))
    }

    async fn fetch_workflow_runs_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, status, parameters, created_at, started_at, finished_at, message FROM workflow_runs WHERE status = ? ORDER BY id")
            .bind(status.as_str())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_run)
            .collect())
    }

    async fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, status, parameters, created_at, started_at, finished_at, message FROM workflow_runs WHERE workflow_id = ? ORDER BY id DESC")
            .bind(workflow_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_run)
            .collect())
    }

    async fn update_workflow_run_status(
        &self,
        workflow_run_id: i64,
        status: RunStatus,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = matches!(
            status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
        );
        self.pool.execute(sqlx::query(
            "UPDATE workflow_runs SET status = ?, message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(message)
        .bind(status.as_str())
        .bind(now)
        .bind(terminal)
        .bind(now)
        .bind(workflow_run_id))
        .await?;
        Ok(())
    }

    async fn create_workflow_step_run(
        &self,
        workflow_run_id: i64,
        step_id: String,
        parameters: Value,
    ) -> Result<WorkflowStepRun, SendableError> {
        let row = sqlx::query(
            "INSERT INTO workflow_step_runs (workflow_run_id, step_id, status, attempt, parameters, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             RETURNING id, workflow_run_id, step_id, task_run_id, status, attempt, parameters, created_at, started_at, finished_at, message",
        )
        .bind(workflow_run_id)
        .bind(step_id)
        .bind(RunStatus::Queued.as_str())
        .bind(0i64)
        .bind(parameters.to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_workflow_step_run(&row))
    }

    async fn update_workflow_step_run(
        &self,
        step_run_id: i64,
        status: RunStatus,
        task_run_id: Option<i64>,
        attempt: Option<i64>,
        parameters: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = matches!(
            status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
        );
        self.pool.execute(sqlx::query(
            "UPDATE workflow_step_runs SET status = ?, task_run_id = CASE WHEN ? = 'queued' THEN NULL ELSE COALESCE(?, task_run_id) END, attempt = COALESCE(?, attempt), parameters = COALESCE(?, parameters), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' THEN ? WHEN ? = 'queued' THEN NULL ELSE started_at END, finished_at = CASE WHEN ? THEN ? WHEN ? = 'queued' THEN NULL ELSE finished_at END WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(status.as_str())
        .bind(task_run_id)
        .bind(attempt)
        .bind(parameters.map(|value| value.to_string()))
        .bind(message)
        .bind(status.as_str())
        .bind(now)
        .bind(status.as_str())
        .bind(terminal)
        .bind(now)
        .bind(status.as_str())
        .bind(step_run_id))
        .await?;
        Ok(())
    }

    async fn fetch_workflow_step_runs(
        &self,
        workflow_run_id: i64,
    ) -> Result<Vec<WorkflowStepRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_run_id, step_id, task_run_id, status, attempt, parameters, created_at, started_at, finished_at, message FROM workflow_step_runs WHERE workflow_run_id = ? ORDER BY id")
            .bind(workflow_run_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_step_run)
            .collect())
    }
}
