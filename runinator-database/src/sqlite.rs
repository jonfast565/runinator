use std::{fs, path::PathBuf};

use chrono::{DateTime, Utc};
use futures_util::stream::StreamExt;
use log::{debug, info};
use runinator_comm::{ActionCommand, WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    notifications::{NewNotification, Notification},
    orchestration::{NewOrchestrationEvent, OrchestrationEvent, ReadyNodeRecord},
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    settings::{SettingKind, SettingRecord},
    workflows::{
        WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact, WorkflowNodeRunChunk,
        WorkflowRun, WorkflowStatus, WorkflowTrigger,
    },
};
use sqlx::{
    ConnectOptions, Executor, Row, SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions,
};

use crate::{
    common::{
        is_trigger_in_blackout, json_metadata, json_opt_i64, json_opt_str, json_str,
        next_execution_for_cron, status_list, trigger_parameters, trigger_state,
        workflow_result_event_type,
    },
    interfaces::DatabaseImpl,
    mappers,
    queries::{self, SqlDialect},
};

static SQLITE_MIGRATOR: Migrator = sqlx::migrate!("./migrations/sqlite");

pub struct SqliteDb {
    pub pool: SqlitePool,
}

#[cfg(test)]
#[path = "sqlite_tests.rs"]
mod tests;

impl SqliteDb {
    pub async fn new(filename: &str) -> Result<Self, SendableError> {
        let options = SqliteConnectOptions::new()
            .filename(filename)
            .create_if_missing(true);
        let options_with_logs = options
            .log_statements(log::LevelFilter::Debug)
            .log_slow_statements(log::LevelFilter::Warn, std::time::Duration::from_secs(1));
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

impl SqliteDb {
    pub async fn run_migrations(&self) -> Result<(), SendableError> {
        info!("Running embedded SQLite migrations");
        SQLITE_MIGRATOR
            .run(&self.pool)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }
}

impl DatabaseImpl for SqliteDb {
    async fn run_init_scripts(&self, paths: &[String]) -> Result<(), SendableError> {
        self.run_migrations().await?;
        for path in paths.iter() {
            let path_info = PathBuf::from(path);
            if path_info.extension().and_then(|ext| ext.to_str()) == Some("sql") {
                info!("Running {}", path_info.display());
                let script = fs::read_to_string(path_info.as_path())?;
                self.execute_script(&script).await?;
            }
        }

        Ok(())
    }

    async fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<RunSummary>, SendableError> {
        let rows = sqlx::query(&queries::fetch_runs_by_status(SqlDialect::Sqlite))
            .bind(status.as_str())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| mappers::sqlite_row_to_run_summary(&row))
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
        self.pool
            .execute(
                sqlx::query(&queries::update_run_status(SqlDialect::Sqlite))
                    .bind(status.as_str())
                    .bind(output_json.map(|v| v.to_string()))
                    .bind(message)
                    .bind(status.as_str())
                    .bind(now)
                    .bind(terminal)
                    .bind(now)
                    .bind(run_id),
            )
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

    async fn fetch_all_artifacts(&self) -> Result<Vec<RunArtifact>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_run_artifact)
            .collect())
    }

    async fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition, SendableError> {
        let now = Utc::now().timestamp();
        let workflow_id = match workflow.id {
            Some(id) => Some(id),
            None => sqlx::query("SELECT id FROM workflows WHERE name = ? ORDER BY id LIMIT 1")
                .bind(&workflow.name)
                .fetch_optional(&self.pool)
                .await?
                .map(|row| row.get::<i64, _>("id")),
        };
        let row = sqlx::query(
            "INSERT INTO workflows (id, name, version, enabled, input_schema, definition, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, version = excluded.version, enabled = excluded.enabled, input_schema = excluded.input_schema, definition = excluded.definition, updated_at = excluded.updated_at
             RETURNING id, name, version, enabled, input_schema, definition, created_at, updated_at",
        )
        .bind(workflow_id)
        .bind(&workflow.name)
        .bind(workflow.version)
        .bind(workflow.enabled)
        .bind(serde_json::to_string(&workflow.input_type)?)
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

    async fn fetch_workflow_by_name(
        &self,
        name: String,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        let row = sqlx::query("SELECT id, name, version, enabled, input_schema, definition, created_at, updated_at FROM workflows WHERE name = ? ORDER BY id LIMIT 1")
            .bind(name)
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

    async fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> Result<WorkflowTrigger, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO workflow_triggers (id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET workflow_id = excluded.workflow_id, kind = excluded.kind, enabled = excluded.enabled, configuration = excluded.configuration, next_execution = excluded.next_execution, blackout_start = excluded.blackout_start, blackout_end = excluded.blackout_end, metadata = excluded.metadata, updated_at = excluded.updated_at
             RETURNING id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at",
        )
        .bind(trigger.id)
        .bind(trigger.workflow_id)
        .bind(trigger.kind.as_str())
        .bind(trigger.enabled)
        .bind(trigger.configuration.to_string())
        .bind(trigger.next_execution.map(|dt| dt.timestamp()))
        .bind(trigger.blackout_start.map(|dt| dt.timestamp()))
        .bind(trigger.blackout_end.map(|dt| dt.timestamp()))
        .bind(trigger.metadata.to_string())
        .bind(trigger.created_at.map(|dt| dt.timestamp()).unwrap_or(now))
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_workflow_trigger(&row))
    }

    async fn fetch_workflow_triggers(
        &self,
        workflow_id: i64,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE workflow_id = ? ORDER BY id")
            .bind(workflow_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_trigger)
            .collect())
    }

    async fn fetch_workflow_trigger(
        &self,
        trigger_id: i64,
    ) -> Result<Option<WorkflowTrigger>, SendableError> {
        let row = sqlx::query("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE id = ?")
            .bind(trigger_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_workflow_trigger(&row)))
    }

    async fn delete_workflow_trigger(&self, trigger_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(sqlx::query("DELETE FROM workflow_triggers WHERE id = ?").bind(trigger_id))
            .await?;
        Ok(())
    }

    async fn fetch_due_workflow_triggers(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE enabled = 1 AND kind = 'cron' AND (next_execution IS NULL OR next_execution <= ?) ORDER BY COALESCE(next_execution, 0), id")
            .bind(now.timestamp())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_trigger)
            .collect())
    }

    async fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: i64,
        next_execution: Option<DateTime<Utc>>,
    ) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query(
                    "UPDATE workflow_triggers SET next_execution = ?, updated_at = ? WHERE id = ?",
                )
                .bind(next_execution.map(|dt| dt.timestamp()))
                .bind(Utc::now().timestamp())
                .bind(trigger_id),
            )
            .await?;
        Ok(())
    }

    async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE enabled = 1 AND kind = 'cron' AND (next_execution IS NULL OR next_execution <= ?) ORDER BY COALESCE(next_execution, 0), id LIMIT ?")
            .bind(now.timestamp())
            .bind(limit.max(1))
            .fetch_all(&mut *tx)
            .await?;

        let mut runs = Vec::new();
        for row in rows {
            let mut trigger = mappers::sqlite_row_to_workflow_trigger(&row);
            let Some(trigger_id) = trigger.id else {
                continue;
            };
            let cron_schedule = trigger
                .configuration
                .get("cron")
                .and_then(Value::as_str)
                .unwrap_or_default();

            if trigger.next_execution.is_none() {
                trigger.next_execution = Some(next_execution_for_cron(cron_schedule, now)?);
                sqlx::query(
                    "UPDATE workflow_triggers SET next_execution = ?, updated_at = ? WHERE id = ?",
                )
                .bind(trigger.next_execution.map(|dt| dt.timestamp()))
                .bind(now.timestamp())
                .bind(trigger_id)
                .execute(&mut *tx)
                .await?;
                continue;
            }

            if is_trigger_in_blackout(&trigger, now) {
                if let Some(end) = trigger.blackout_end {
                    sqlx::query("UPDATE workflow_triggers SET next_execution = ?, updated_at = ? WHERE id = ?")
                        .bind(end.timestamp())
                        .bind(now.timestamp())
                        .bind(trigger_id)
                        .execute(&mut *tx)
                        .await?;
                }
                continue;
            }

            let fire_key = trigger
                .next_execution
                .map(|dt| dt.timestamp().to_string())
                .unwrap_or_else(|| "initial".into());
            let insert = sqlx::query(
                "INSERT OR IGNORE INTO workflow_trigger_firings (trigger_id, fire_key, scheduler_id, created_at)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(trigger_id)
            .bind(&fire_key)
            .bind(&scheduler_id)
            .bind(now.timestamp())
            .execute(&mut *tx)
            .await?;
            if insert.rows_affected() == 0 {
                continue;
            }

            let workflow_row = sqlx::query("SELECT id, name, version, enabled, input_schema, definition, created_at, updated_at FROM workflows WHERE id = ?")
                .bind(trigger.workflow_id)
                .fetch_one(&mut *tx)
                .await?;
            let workflow_snapshot = mappers::sqlite_row_to_workflow(&workflow_row);
            let run_row = sqlx::query(
                "INSERT INTO workflow_runs (workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name)
                 VALUES (?, ?, ?, NULL, ?, ?, ?, NULL)
                 RETURNING id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name",
            )
            .bind(trigger.workflow_id)
            .bind(serde_json::to_string(&workflow_snapshot)?)
            .bind(WorkflowStatus::Queued.as_str())
            .bind(trigger_parameters(&trigger).to_string())
            .bind(trigger_state(&trigger).to_string())
            .bind(now.timestamp())
            .fetch_one(&mut *tx)
            .await?;
            let run = mappers::sqlite_row_to_workflow_run(&run_row);

            sqlx::query("UPDATE workflow_trigger_firings SET workflow_run_id = ? WHERE trigger_id = ? AND fire_key = ?")
                .bind(run.id)
                .bind(trigger_id)
                .bind(&fire_key)
                .execute(&mut *tx)
                .await?;

            let next_execution = next_execution_for_cron(cron_schedule, now)?;
            sqlx::query(
                "UPDATE workflow_triggers SET next_execution = ?, updated_at = ? WHERE id = ?",
            )
            .bind(next_execution.timestamp())
            .bind(now.timestamp())
            .bind(trigger_id)
            .execute(&mut *tx)
            .await?;
            runs.push(run);
        }

        tx.commit().await?;
        Ok(runs)
    }

    async fn create_workflow_run(
        &self,
        workflow_id: i64,
        workflow_snapshot: WorkflowDefinition,
        parameters: Value,
        state: Value,
        name: Option<String>,
    ) -> Result<WorkflowRun, SendableError> {
        let row = sqlx::query(
            "INSERT INTO workflow_runs (workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, name)
             VALUES (?, ?, ?, NULL, ?, ?, ?, ?)
             RETURNING id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name",
        )
        .bind(workflow_id)
        .bind(serde_json::to_string(&workflow_snapshot)?)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(state.to_string())
        .bind(Utc::now().timestamp())
        .bind(name)
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_workflow_run(&row))
    }

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<Option<WorkflowRun>, SendableError> {
        let row = sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs WHERE id = ?")
            .bind(workflow_run_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_workflow_run(&row)))
    }

    async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs WHERE status = ? ORDER BY id")
            .bind(status.as_str())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_run)
            .collect())
    }

    async fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: String,
        statuses: Vec<WorkflowStatus>,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        if statuses.is_empty() {
            return Ok(Vec::new());
        }
        let statuses = status_list(&statuses);
        let sql = queries::claim_workflow_runs_for_scheduler(SqlDialect::Sqlite, &statuses);
        let rows = sqlx::query(&sql)
            .bind(&scheduler_id)
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(&scheduler_id)
            .bind(limit.max(1))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_run)
            .collect())
    }

    async fn renew_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: String,
        lease_until: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(
            "UPDATE workflow_runs SET scheduler_claimed_until = ? WHERE id = ? AND scheduler_claimed_by = ?",
        )
        .bind(lease_until.timestamp())
        .bind(workflow_run_id)
        .bind(scheduler_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn release_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: String,
    ) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query(
                    "UPDATE workflow_runs SET scheduler_claimed_by = NULL, scheduler_claimed_until = NULL WHERE id = ? AND scheduler_claimed_by = ?",
                )
                .bind(workflow_run_id)
                .bind(scheduler_id),
            )
            .await?;
        Ok(())
    }

    async fn fetch_recent_workflow_runs(&self) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs ORDER BY id DESC")
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
        let rows = sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs WHERE workflow_id = ? ORDER BY id DESC")
            .bind(workflow_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_run)
            .collect())
    }

    async fn fetch_workflow_runs_by_name(
        &self,
        name: String,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = if open_only {
            sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs WHERE name = ? AND status NOT IN ('succeeded', 'failed', 'timed_out', 'canceled') ORDER BY id DESC")
                .bind(name)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs WHERE name = ? ORDER BY id DESC")
                .bind(name)
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_run)
            .collect())
    }

    async fn set_workflow_run_name(
        &self,
        workflow_run_id: i64,
        name: Option<String>,
    ) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query("UPDATE workflow_runs SET name = ? WHERE id = ?")
                    .bind(name)
                    .bind(workflow_run_id),
            )
            .await?;
        Ok(())
    }

    async fn update_workflow_run_status(
        &self,
        workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        self.pool.execute(sqlx::query(
            "UPDATE workflow_runs SET status = ?, active_node_id = COALESCE(?, active_node_id), state = COALESCE(?, state), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN ? ELSE started_at END, finished_at = CASE WHEN ? THEN ? ELSE finished_at END WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(active_node_id)
        .bind(state.map(|value| value.to_string()))
        .bind(message)
        .bind(status.as_str())
        .bind(now)
        .bind(terminal)
        .bind(now)
        .bind(workflow_run_id))
        .await?;
        Ok(())
    }

    async fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: String,
        parameters: Value,
    ) -> Result<WorkflowNodeRun, SendableError> {
        let row = sqlx::query(
            "INSERT INTO workflow_node_runs (workflow_run_id, node_id, status, attempt, parameters, state, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             RETURNING id, workflow_run_id, node_id, status, attempt, parameters, output_json, state, transition_reason, created_at, started_at, finished_at, message",
        )
        .bind(workflow_run_id)
        .bind(node_id)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(0i64)
        .bind(parameters.to_string())
        .bind(Value::Object(Default::default()).to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_workflow_node_run(&row))
    }

    async fn update_workflow_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        let terminal = status.is_terminal();
        self.pool.execute(sqlx::query(
            "UPDATE workflow_node_runs SET status = ?, attempt = COALESCE(?, attempt), parameters = COALESCE(?, parameters), output_json = COALESCE(?, output_json), state = COALESCE(?, state), transition_reason = COALESCE(?, transition_reason), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' THEN ? WHEN ? = 'queued' THEN NULL ELSE started_at END, finished_at = CASE WHEN ? THEN ? WHEN ? = 'queued' THEN NULL ELSE finished_at END WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(attempt)
        .bind(parameters.map(|value| value.to_string()))
        .bind(output_json.map(|value| value.to_string()))
        .bind(state.map(|value| value.to_string()))
        .bind(transition_reason)
        .bind(message)
        .bind(status.as_str())
        .bind(now)
        .bind(status.as_str())
        .bind(terminal)
        .bind(now)
        .bind(status.as_str())
        .bind(node_run_id))
        .await?;
        Ok(())
    }

    async fn fetch_workflow_node_runs(
        &self,
        workflow_run_id: i64,
    ) -> Result<Vec<WorkflowNodeRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_run_id, node_id, status, attempt, parameters, output_json, state, transition_reason, created_at, started_at, finished_at, message FROM workflow_node_runs WHERE workflow_run_id = ? ORDER BY id")
            .bind(workflow_run_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| mappers::sqlite_row_to_workflow_node_run(&row))
            .collect())
    }

    async fn fetch_workflow_node_run(
        &self,
        workflow_node_run_id: i64,
    ) -> Result<Option<WorkflowNodeRun>, SendableError> {
        let row = sqlx::query("SELECT id, workflow_run_id, node_id, status, attempt, parameters, output_json, state, transition_reason, created_at, started_at, finished_at, message FROM workflow_node_runs WHERE id = ?")
            .bind(workflow_node_run_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_workflow_node_run(&row)))
    }

    async fn append_workflow_node_run_chunk(
        &self,
        workflow_node_run_id: i64,
        chunk: &NewRunChunk,
    ) -> Result<WorkflowNodeRunChunk, SendableError> {
        let sequence: i64 = sqlx::query("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM workflow_node_chunks WHERE workflow_node_run_id = ?")
            .bind(workflow_node_run_id)
            .fetch_one(&self.pool)
            .await?
            .get("next_sequence");
        let row = sqlx::query(
            "INSERT INTO workflow_node_chunks (workflow_node_run_id, sequence, stream, content, created_at)
             VALUES (?, ?, ?, ?, ?)
             RETURNING id, workflow_node_run_id, sequence, stream, content, created_at",
        )
        .bind(workflow_node_run_id)
        .bind(sequence)
        .bind(&chunk.stream)
        .bind(&chunk.content)
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_workflow_node_run_chunk(&row))
    }

    async fn fetch_workflow_node_run_chunks(
        &self,
        workflow_node_run_id: i64,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<WorkflowNodeRunChunk>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, workflow_node_run_id, sequence, stream, content, created_at FROM workflow_node_chunks WHERE workflow_node_run_id = ? AND sequence > ? ORDER BY sequence ASC LIMIT ?",
        )
        .bind(workflow_node_run_id)
        .bind(cursor.unwrap_or(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_node_run_chunk)
            .collect())
    }

    async fn add_workflow_node_run_artifact(
        &self,
        workflow_node_run_id: i64,
        artifact: &NewRunArtifact,
    ) -> Result<WorkflowNodeRunArtifact, SendableError> {
        let row = sqlx::query(
            "INSERT INTO workflow_node_artifacts (workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             RETURNING id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at",
        )
        .bind(workflow_node_run_id)
        .bind(&artifact.name)
        .bind(&artifact.mime_type)
        .bind(artifact.size_bytes)
        .bind(&artifact.uri)
        .bind(artifact.metadata.to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_workflow_node_run_artifact(&row))
    }

    async fn fetch_workflow_node_run_artifacts(
        &self,
        workflow_node_run_id: i64,
    ) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM workflow_node_artifacts WHERE workflow_node_run_id = ? ORDER BY id ASC",
        )
        .bind(workflow_node_run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_workflow_node_run_artifact)
            .collect())
    }

    async fn apply_workflow_result_event(
        &self,
        event: &WorkflowResultEvent,
    ) -> Result<bool, SendableError> {
        let mut tx = self.pool.begin().await?;
        let event_type = workflow_result_event_type(event);
        let insert = sqlx::query(
            "INSERT OR IGNORE INTO workflow_result_events (event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(event.event_id.to_string())
        .bind(event.workflow_run_id)
        .bind(event.workflow_node_run_id)
        .bind(&event.node_id)
        .bind(event_type)
        .bind(event.timestamp.timestamp())
        .execute(&mut *tx)
        .await?;

        if insert.rows_affected() == 0 {
            tx.commit().await?;
            return Ok(false);
        }

        match &event.kind {
            WorkflowResultEventKind::Status {
                status,
                output_json,
                message,
            } => {
                let now = Utc::now().timestamp();
                let terminal = status.is_terminal();
                sqlx::query(
                    "UPDATE workflow_node_runs SET status = ?, output_json = COALESCE(?, output_json), message = COALESCE(?, message), started_at = CASE WHEN ? = 'running' THEN ? WHEN ? = 'queued' THEN NULL ELSE started_at END, finished_at = CASE WHEN ? THEN ? WHEN ? = 'queued' THEN NULL ELSE finished_at END WHERE id = ? AND NOT (status IN ('succeeded', 'failed', 'timed_out', 'canceled') AND ? NOT IN ('succeeded', 'failed', 'timed_out', 'canceled'))",
                )
                .bind(status.as_str())
                .bind(output_json.as_ref().map(|value: &Value| value.to_string()))
                .bind(message)
                .bind(status.as_str())
                .bind(now)
                .bind(status.as_str())
                .bind(terminal)
                .bind(now)
                .bind(status.as_str())
                .bind(event.workflow_node_run_id)
                .bind(status.as_str())
                .execute(&mut *tx)
                .await?;
            }
            WorkflowResultEventKind::Chunk { chunk } => {
                let sequence: i64 = sqlx::query("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM workflow_node_chunks WHERE workflow_node_run_id = ?")
                    .bind(event.workflow_node_run_id)
                    .fetch_one(&mut *tx)
                    .await?
                    .get("next_sequence");
                sqlx::query(
                    "INSERT INTO workflow_node_chunks (workflow_node_run_id, sequence, stream, content, created_at)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(event.workflow_node_run_id)
                .bind(sequence)
                .bind(&chunk.stream)
                .bind(&chunk.content)
                .bind(event.timestamp.timestamp())
                .execute(&mut *tx)
                .await?;
            }
            WorkflowResultEventKind::Artifact { artifact } => {
                sqlx::query(
                    "INSERT INTO workflow_node_artifacts (workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(event.workflow_node_run_id)
                .bind(&artifact.name)
                .bind(&artifact.mime_type)
                .bind(artifact.size_bytes)
                .bind(&artifact.uri)
                .bind(artifact.metadata.to_string())
                .bind(event.timestamp.timestamp())
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(true)
    }

    async fn append_orchestration_event(
        &self,
        event: &NewOrchestrationEvent,
    ) -> Result<bool, SendableError> {
        let insert = sqlx::query(
            "INSERT OR IGNORE INTO workflow_orchestration_events (event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, payload, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(event.event_id.to_string())
        .bind(event.workflow_run_id)
        .bind(event.workflow_node_run_id)
        .bind(&event.node_id)
        .bind(&event.event_type)
        .bind(event.payload.to_string())
        .bind(event.created_at.timestamp())
        .execute(&self.pool)
        .await?;
        Ok(insert.rows_affected() > 0)
    }

    async fn fetch_orchestration_events(
        &self,
        workflow_run_id: i64,
        limit: i64,
    ) -> Result<Vec<OrchestrationEvent>, SendableError> {
        let rows = sqlx::query(
            "SELECT event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, payload, created_at
             FROM workflow_orchestration_events
             WHERE workflow_run_id = ?
             ORDER BY created_at, event_id
             LIMIT ?",
        )
        .bind(workflow_run_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(mappers::sqlite_row_to_orchestration_event)
            .collect()
    }

    async fn enqueue_ready_node(
        &self,
        event: NewOrchestrationEvent,
        node_id: String,
        ready_at: DateTime<Utc>,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let mut tx = self.pool.begin().await?;
        let inserted_event = sqlx::query(
            "INSERT OR IGNORE INTO workflow_orchestration_events (event_id, workflow_run_id, workflow_node_run_id, node_id, event_type, payload, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(event.event_id.to_string())
        .bind(event.workflow_run_id)
        .bind(event.workflow_node_run_id)
        .bind(&event.node_id)
        .bind(&event.event_type)
        .bind(event.payload.to_string())
        .bind(event.created_at.timestamp())
        .execute(&mut *tx)
        .await?;
        if inserted_event.rows_affected() == 0 {
            tx.commit().await?;
            return Ok(None);
        }

        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT OR IGNORE INTO workflow_ready_nodes (source_event_id, workflow_run_id, node_id, status, ready_at, attempts, created_at, updated_at)
             VALUES (?, ?, ?, 'queued', ?, 0, ?, ?)
             RETURNING id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at",
        )
        .bind(event.event_id.to_string())
        .bind(event.workflow_run_id)
        .bind(node_id)
        .bind(ready_at.timestamp())
        .bind(now)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        row.as_ref()
            .map(mappers::sqlite_row_to_ready_node)
            .transpose()
    }

    async fn claim_ready_nodes(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>, SendableError> {
        let rows = sqlx::query(
            "UPDATE workflow_ready_nodes
             SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
             WHERE id IN (
                 SELECT id FROM workflow_ready_nodes
                 WHERE completed_at IS NULL
                   AND ready_at <= ?
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                 ORDER BY ready_at, id
                 LIMIT ?
             )
             RETURNING id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at",
        )
        .bind(&scheduler_id)
        .bind(lease_until.timestamp())
        .bind(now.timestamp())
        .bind(now.timestamp())
        .bind(now.timestamp())
        .bind(&scheduler_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(mappers::sqlite_row_to_ready_node).collect()
    }

    async fn fetch_ready_node(
        &self,
        ready_node_id: i64,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let row = sqlx::query(
            "SELECT id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at
             FROM workflow_ready_nodes
             WHERE id = ?",
        )
        .bind(ready_node_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref()
            .map(mappers::sqlite_row_to_ready_node)
            .transpose()
    }

    async fn complete_ready_node(
        &self,
        ready_node_id: i64,
        scheduler_id: String,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(
            "UPDATE workflow_ready_nodes
             SET completed_at = ?, status = 'succeeded', updated_at = ?
             WHERE id = ? AND claimed_by = ?",
        )
        .bind(now)
        .bind(now)
        .bind(ready_node_id)
        .bind(scheduler_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn fetch_pending_ready_nodes(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at
             FROM workflow_ready_nodes
             WHERE completed_at IS NULL
               AND (claimed_until IS NULL OR claimed_until <= ?)
             ORDER BY ready_at, id
             LIMIT ?",
        )
        .bind(now.timestamp())
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(mappers::sqlite_row_to_ready_node).collect()
    }

    async fn claim_ready_node(
        &self,
        ready_node_id: i64,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> Result<Option<ReadyNodeRecord>, SendableError> {
        let row = sqlx::query(
            "UPDATE workflow_ready_nodes
             SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1, status = 'running', updated_at = ?
             WHERE id = ?
               AND completed_at IS NULL
               AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
             RETURNING id, source_event_id, workflow_run_id, node_id, status, ready_at, attempts, claimed_by, claimed_until, completed_at, created_at, updated_at",
        )
        .bind(&scheduler_id)
        .bind(lease_until.timestamp())
        .bind(now.timestamp())
        .bind(ready_node_id)
        .bind(now.timestamp())
        .bind(&scheduler_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref()
            .map(mappers::sqlite_row_to_ready_node)
            .transpose()
    }

    async fn release_ready_node(
        &self,
        ready_node_id: i64,
        scheduler_id: String,
    ) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(
            "UPDATE workflow_ready_nodes
             SET claimed_by = NULL, claimed_until = NULL, status = 'queued', updated_at = ?
             WHERE id = ? AND claimed_by = ? AND completed_at IS NULL",
        )
        .bind(now)
        .bind(ready_node_id)
        .bind(scheduler_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn upsert_catalog_item(&self, item: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO catalog_items (uri, item_type, name, version, document, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(uri) DO UPDATE SET item_type = excluded.item_type, name = excluded.name, version = excluded.version, document = excluded.document, metadata = excluded.metadata, updated_at = excluded.updated_at
             RETURNING id, uri, item_type, name, version, document, metadata, created_at, updated_at",
        )
        .bind(json_str(&item, "uri"))
        .bind(json_str(&item, "item_type"))
        .bind(json_str(&item, "name"))
        .bind(json_str(&item, "version"))
        .bind(item.get("document").cloned().unwrap_or(Value::Object(Default::default())).to_string())
        .bind(json_metadata(&item))
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_catalog_item(&row))
    }

    async fn fetch_catalog_items(
        &self,
        item_type: Option<String>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = if let Some(item_type) = item_type {
            sqlx::query("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items WHERE item_type = ? ORDER BY uri")
                .bind(item_type)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items ORDER BY uri")
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_catalog_item)
            .collect())
    }

    async fn fetch_catalog_item(&self, uri: String) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items WHERE uri = ?")
            .bind(uri)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_catalog_item(&row)))
    }

    async fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO automation_records (record_type, workflow_run_id, external_item_id, node_id, provider, resource_type, external_id, status, title, url, body, path, prompt, approval_type, resolved_by, resolved_at, metadata, data, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, record_type, data, created_at, updated_at",
        )
        .bind(record_type)
        .bind(json_opt_i64(&record, "workflow_run_id"))
        .bind(json_opt_i64(&record, "external_item_id"))
        .bind(json_opt_str(&record, "node_id"))
        .bind(json_str(&record, "provider"))
        .bind(json_str(&record, "resource_type"))
        .bind(json_str(&record, "external_id"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "title"))
        .bind(json_opt_str(&record, "url"))
        .bind(json_opt_str(&record, "body"))
        .bind(json_opt_str(&record, "path"))
        .bind(json_opt_str(&record, "prompt"))
        .bind(json_opt_str(&record, "approval_type"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_automation_record(&row))
    }

    async fn update_automation_record(
        &self,
        record_type: String,
        record_id: i64,
        record: Value,
    ) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "UPDATE automation_records SET workflow_run_id = ?, external_item_id = ?, node_id = ?, provider = ?, resource_type = ?, external_id = ?, status = ?, title = ?, url = ?, body = ?, path = ?, prompt = ?, approval_type = ?, resolved_by = ?, resolved_at = ?, metadata = ?, data = ?, updated_at = ? WHERE id = ? AND record_type = ? RETURNING id, record_type, data, created_at, updated_at",
        )
        .bind(json_opt_i64(&record, "workflow_run_id"))
        .bind(json_opt_i64(&record, "external_item_id"))
        .bind(json_opt_str(&record, "node_id"))
        .bind(json_str(&record, "provider"))
        .bind(json_str(&record, "resource_type"))
        .bind(json_str(&record, "external_id"))
        .bind(json_str(&record, "status"))
        .bind(json_opt_str(&record, "title"))
        .bind(json_opt_str(&record, "url"))
        .bind(json_opt_str(&record, "body"))
        .bind(json_opt_str(&record, "path"))
        .bind(json_opt_str(&record, "prompt"))
        .bind(json_opt_str(&record, "approval_type"))
        .bind(json_opt_str(&record, "resolved_by"))
        .bind(json_opt_i64(&record, "resolved_at"))
        .bind(json_metadata(&record))
        .bind(record.to_string())
        .bind(now)
        .bind(record_id)
        .bind(record_type)
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_automation_record(&row))
    }

    async fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<i64>,
        external_item_id: Option<i64>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = sqlx::query("SELECT id, record_type, data, created_at, updated_at FROM automation_records WHERE record_type = ? ORDER BY id DESC")
            .bind(record_type)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_automation_record)
            .filter(|record| {
                workflow_run_id.is_none_or(|id| {
                    record.get("workflow_run_id").and_then(Value::as_i64) == Some(id)
                }) && external_item_id.is_none_or(|id| {
                    record.get("external_item_id").and_then(Value::as_i64) == Some(id)
                })
            })
            .collect())
    }

    async fn fetch_automation_record(
        &self,
        record_type: String,
        record_id: i64,
    ) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query("SELECT id, record_type, data, created_at, updated_at FROM automation_records WHERE id = ? AND record_type = ?")
            .bind(record_id)
            .bind(record_type)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_automation_record(&row)))
    }

    async fn put_idempotency_key(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> Result<Value, SendableError> {
        let row = sqlx::query(
            "INSERT INTO idempotency_keys (scope, key, result, created_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(scope, key) DO UPDATE SET result = idempotency_keys.result
             RETURNING id, scope, key, result, created_at",
        )
        .bind(scope)
        .bind(key)
        .bind(result.to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_idempotency_key(&row))
    }

    async fn fetch_idempotency_key(
        &self,
        scope: String,
        key: String,
    ) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query("SELECT id, scope, key, result, created_at FROM idempotency_keys WHERE scope = ? AND key = ?")
            .bind(scope)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_idempotency_key(&row)))
    }

    async fn enqueue_action_dispatch(
        &self,
        dedupe_key: String,
        command: ActionCommand,
    ) -> Result<runinator_comm::ActionDispatchRecord, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO workflow_action_dispatches (dedupe_key, command_json, attempts, created_at, updated_at)
             VALUES (?, ?, 0, ?, ?)
             ON CONFLICT(dedupe_key) DO UPDATE SET command_json = workflow_action_dispatches.command_json
             RETURNING id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until",
        )
        .bind(dedupe_key)
        .bind(serde_json::to_string(&command)?)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        mappers::sqlite_row_to_action_dispatch(&row)
    }

    async fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> Result<Vec<runinator_comm::ActionDispatchRecord>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until
             FROM workflow_action_dispatches
             WHERE published_at IS NULL
             ORDER BY updated_at ASC, id ASC
             LIMIT ?",
        )
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(mappers::sqlite_row_to_action_dispatch)
            .collect()
    }

    async fn claim_pending_action_dispatches(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<runinator_comm::ActionDispatchRecord>, SendableError> {
        let rows = sqlx::query(
            "UPDATE workflow_action_dispatches
             SET claimed_by = ?, claimed_until = ?, updated_at = ?
             WHERE id IN (
                 SELECT id FROM workflow_action_dispatches
                 WHERE published_at IS NULL
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                 ORDER BY updated_at ASC, id ASC
                 LIMIT ?
             )
             RETURNING id, dedupe_key, command_json, attempts, created_at, updated_at, published_at, last_error, claimed_by, claimed_until",
        )
        .bind(&scheduler_id)
        .bind(lease_until.timestamp())
        .bind(now.timestamp())
        .bind(now.timestamp())
        .bind(&scheduler_id)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(mappers::sqlite_row_to_action_dispatch)
            .collect()
    }

    async fn mark_action_dispatch_published(&self, dispatch_id: i64) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE workflow_action_dispatches
             SET published_at = ?, updated_at = ?, last_error = NULL, claimed_by = NULL, claimed_until = NULL
             WHERE id = ?",
        )
        .bind(now)
        .bind(now)
        .bind(dispatch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_action_dispatch_failed(
        &self,
        dispatch_id: i64,
        error: String,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE workflow_action_dispatches
             SET attempts = attempts + 1, updated_at = ?, last_error = ?, claimed_by = NULL, claimed_until = NULL
             WHERE id = ?",
        )
        .bind(now)
        .bind(error)
        .bind(dispatch_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_notification(
        &self,
        notification: &NewNotification,
    ) -> Result<Notification, SendableError> {
        let row = sqlx::query(
            "INSERT INTO notifications (workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at",
        )
        .bind(notification.workflow_run_id)
        .bind(notification.workflow_node_id.as_ref())
        .bind(&notification.channel)
        .bind(&notification.severity)
        .bind(&notification.title)
        .bind(notification.body.as_ref())
        .bind(notification.target.as_ref())
        .bind(notification.metadata.to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::sqlite_row_to_notification(&row))
    }

    async fn fetch_notifications(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> Result<Vec<Notification>, SendableError> {
        let bounded_limit = limit.clamp(1, 1000);
        let rows = if unread_only {
            sqlx::query(
                "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications WHERE read_at IS NULL ORDER BY created_at DESC LIMIT ?",
            )
            .bind(bounded_limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications ORDER BY created_at DESC LIMIT ?",
            )
            .bind(bounded_limit)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .iter()
            .map(mappers::sqlite_row_to_notification)
            .collect())
    }

    async fn mark_notification_read(
        &self,
        notification_id: i64,
    ) -> Result<Option<Notification>, SendableError> {
        let row = sqlx::query(
            "UPDATE notifications SET read_at = COALESCE(read_at, ?) WHERE id = ? RETURNING id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at",
        )
        .bind(Utc::now().timestamp())
        .bind(notification_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_notification(&row)))
    }

    async fn mark_all_notifications_read(&self) -> Result<u64, SendableError> {
        let result = sqlx::query("UPDATE notifications SET read_at = ? WHERE read_at IS NULL")
            .bind(Utc::now().timestamp())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    async fn upsert_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
        value: Vec<u8>,
        updated_at: i64,
    ) -> Result<(), SendableError> {
        sqlx::query(&queries::upsert_setting(SqlDialect::Sqlite))
            .bind(kind.as_str())
            .bind(scope)
            .bind(name)
            .bind(value)
            .bind(updated_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn fetch_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<Option<SettingRecord>, SendableError> {
        let row = sqlx::query(&queries::fetch_setting(SqlDialect::Sqlite))
            .bind(kind.as_str())
            .bind(scope)
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::sqlite_row_to_setting(&row)))
    }

    async fn delete_setting(
        &self,
        kind: SettingKind,
        scope: String,
        name: String,
    ) -> Result<(), SendableError> {
        sqlx::query(&queries::delete_setting(SqlDialect::Sqlite))
            .bind(kind.as_str())
            .bind(scope)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_settings(&self) -> Result<Vec<SettingRecord>, SendableError> {
        let rows = sqlx::query(queries::list_settings())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(mappers::sqlite_row_to_setting).collect())
    }
}
