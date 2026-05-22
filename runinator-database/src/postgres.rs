use std::{fs, path::PathBuf, str::FromStr};

use chrono::{DateTime, Duration, Utc};
use futures_util::stream::StreamExt;
use log::{debug, info};
use runinator_models::{
    errors::SendableError,
    notifications::{NewNotification, Notification},
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    workflows::{
        WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact, WorkflowNodeRunChunk,
        WorkflowRun, WorkflowStatus, WorkflowTrigger,
    },
};
use serde_json::Value;
use sqlx::{
    ConnectOptions, Executor, PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::{interfaces::DatabaseImpl, mappers};

const POSTGRES_TABLE_INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS runs (
    id BIGSERIAL PRIMARY KEY,
    status TEXT NOT NULL,
    parameters TEXT NOT NULL,
    output_json TEXT NULL,
    message TEXT NULL,
    trigger TEXT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    workflow_run_id BIGINT NULL,
    workflow_node_id TEXT NULL
);

CREATE TABLE IF NOT EXISTS run_chunks (
    id BIGSERIAL PRIMARY KEY,
    run_id BIGINT NOT NULL REFERENCES runs(id),
    sequence BIGINT NOT NULL,
    stream TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS run_artifacts (
    id BIGSERIAL PRIMARY KEY,
    run_id BIGINT NOT NULL REFERENCES runs(id),
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    metadata TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflows (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    version BIGINT NOT NULL,
    enabled BOOLEAN NOT NULL,
    input_schema TEXT NOT NULL,
    definition TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_triggers (
    id BIGSERIAL PRIMARY KEY,
    workflow_id BIGINT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    configuration TEXT NOT NULL DEFAULT '{}',
    next_execution BIGINT NULL,
    blackout_start BIGINT NULL,
    blackout_end BIGINT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_runs (
    id BIGSERIAL PRIMARY KEY,
    workflow_id BIGINT NOT NULL REFERENCES workflows(id),
    workflow_snapshot TEXT NULL,
    status TEXT NOT NULL,
    active_node_id TEXT NULL,
    parameters TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    message TEXT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_runs (
    id BIGSERIAL PRIMARY KEY,
    workflow_run_id BIGINT NOT NULL REFERENCES workflow_runs(id),
    node_id TEXT NOT NULL,
    status TEXT NOT NULL,
    attempt BIGINT NOT NULL DEFAULT 0,
    parameters TEXT NOT NULL DEFAULT '{}',
    output_json TEXT NULL,
    state TEXT NOT NULL DEFAULT '{}',
    transition_reason TEXT NULL,
    created_at BIGINT NOT NULL,
    started_at BIGINT NULL,
    finished_at BIGINT NULL,
    message TEXT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_chunks (
    id BIGSERIAL PRIMARY KEY,
    workflow_node_run_id BIGINT NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL,
    stream TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS workflow_node_artifacts (
    id BIGSERIAL PRIMARY KEY,
    workflow_node_run_id BIGINT NOT NULL REFERENCES workflow_node_runs(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    uri TEXT NOT NULL,
    metadata TEXT NOT NULL,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS catalog_items (
    id BIGSERIAL PRIMARY KEY,
    uri TEXT NOT NULL UNIQUE,
    item_type TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    document TEXT NOT NULL DEFAULT '{}',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS automation_records (
    id BIGSERIAL PRIMARY KEY,
    record_type TEXT NOT NULL,
    workflow_run_id BIGINT NULL,
    external_item_id BIGINT NULL,
    node_id TEXT NULL,
    provider TEXT NOT NULL DEFAULT '',
    resource_type TEXT NOT NULL DEFAULT '',
    external_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT '',
    title TEXT NULL,
    url TEXT NULL,
    body TEXT NULL,
    path TEXT NULL,
    prompt TEXT NULL,
    approval_type TEXT NULL,
    resolved_by TEXT NULL,
    resolved_at BIGINT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    data TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    id BIGSERIAL PRIMARY KEY,
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    result TEXT NOT NULL DEFAULT '{}',
    created_at BIGINT NOT NULL,
    UNIQUE(scope, key)
);

CREATE TABLE IF NOT EXISTS notifications (
    id BIGSERIAL PRIMARY KEY,
    workflow_run_id BIGINT NULL,
    workflow_node_id TEXT NULL,
    channel TEXT NOT NULL,
    severity TEXT NOT NULL DEFAULT 'info',
    title TEXT NOT NULL,
    body TEXT NULL,
    target TEXT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    read_at BIGINT NULL,
    created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(read_at, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status);
CREATE INDEX IF NOT EXISTS idx_run_chunks_run_sequence ON run_chunks(run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_status ON workflow_runs(status);
CREATE INDEX IF NOT EXISTS idx_workflow_triggers_workflow ON workflow_triggers(workflow_id);
CREATE INDEX IF NOT EXISTS idx_workflow_triggers_due ON workflow_triggers(enabled, kind, next_execution);
CREATE INDEX IF NOT EXISTS idx_workflow_node_runs_workflow_run ON workflow_node_runs(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_node_chunks_node_sequence ON workflow_node_chunks(workflow_node_run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflow_node_artifacts_node ON workflow_node_artifacts(workflow_node_run_id);
CREATE INDEX IF NOT EXISTS idx_catalog_items_type ON catalog_items(item_type);
CREATE INDEX IF NOT EXISTS idx_automation_records_type ON automation_records(record_type);
CREATE INDEX IF NOT EXISTS idx_automation_records_workflow_run ON automation_records(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_automation_records_external_item ON automation_records(external_item_id);

COMMIT;
"#;

pub struct PostgresDb {
    pub pool: PgPool,
}

impl PostgresDb {
    pub async fn new(connection_str: &str) -> Result<Self, SendableError> {
        let options = PgConnectOptions::from_str(connection_str)?
            .log_statements(log::LevelFilter::Debug)
            .log_slow_statements(
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

fn json_str(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn json_opt_str(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn json_opt_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn json_metadata(value: &Value) -> String {
    value
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()))
        .to_string()
}

impl DatabaseImpl for PostgresDb {
    async fn run_init_scripts(&self, paths: &Vec<String>) -> Result<(), SendableError> {
        info!("Running embedded PostgreSQL table initialization script");
        self.execute_script(POSTGRES_TABLE_INIT_SQL).await?;
        self.execute_script(
            "ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS workflow_snapshot TEXT NULL;",
        )
        .await?;
        self.execute_script("ALTER TABLE workflow_runs ADD COLUMN IF NOT EXISTS name TEXT NULL;")
            .await?;
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

    async fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<RunSummary>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, status, parameters, output_json, message, trigger, started_at, finished_at, created_at, workflow_run_id, workflow_node_id FROM runs WHERE status = $1 ORDER BY id",
        )
        .bind(status.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| mappers::postgres_row_to_run_summary(&row))
            .collect())
    }

    async fn update_run_status(
        &self,
        run_id: i64,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let terminal = matches!(
            status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::TimedOut | RunStatus::Canceled
        );
        let now = Utc::now().timestamp();
        self.pool.execute(sqlx::query(
            "UPDATE runs SET status = $1, output_json = COALESCE($2, output_json), message = COALESCE($3, message), started_at = CASE WHEN $4 = 'running' AND started_at IS NULL THEN $5 ELSE started_at END, finished_at = CASE WHEN $6 THEN $7 ELSE finished_at END WHERE id = $8",
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
        let sequence: i64 = sqlx::query("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM run_chunks WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(&self.pool)
            .await?
            .get("next_sequence");
        let row = sqlx::query(
            "INSERT INTO run_chunks (run_id, sequence, stream, content, created_at)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, run_id, sequence, stream, content, created_at",
        )
        .bind(run_id)
        .bind(sequence)
        .bind(&chunk.stream)
        .bind(&chunk.content)
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::postgres_row_to_run_chunk(&row))
    }

    async fn fetch_run_chunks(
        &self,
        run_id: i64,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<RunChunk>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, run_id, sequence, stream, content, created_at FROM run_chunks WHERE run_id = $1 AND sequence > $2 ORDER BY sequence ASC LIMIT $3",
        )
        .bind(run_id)
        .bind(cursor.unwrap_or(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_run_chunk)
            .collect())
    }

    async fn add_run_artifact(
        &self,
        run_id: i64,
        artifact: &NewRunArtifact,
    ) -> Result<RunArtifact, SendableError> {
        let row = sqlx::query(
            "INSERT INTO run_artifacts (run_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
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
        Ok(mappers::postgres_row_to_run_artifact(&row))
    }

    async fn fetch_run_artifacts(&self, run_id: i64) -> Result<Vec<RunArtifact>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts WHERE run_id = $1 ORDER BY id ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_run_artifact)
            .collect())
    }

    async fn fetch_artifact(&self, artifact_id: i64) -> Result<Option<RunArtifact>, SendableError> {
        let row = sqlx::query(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts WHERE id = $1",
        )
        .bind(artifact_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| mappers::postgres_row_to_run_artifact(&row)))
    }

    async fn fetch_all_artifacts(&self) -> Result<Vec<RunArtifact>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM run_artifacts ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_run_artifact)
            .collect())
    }

    async fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO workflows (id, name, version, enabled, input_schema, definition, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT(id) DO UPDATE SET name = EXCLUDED.name, version = EXCLUDED.version, enabled = EXCLUDED.enabled, input_schema = EXCLUDED.input_schema, definition = EXCLUDED.definition, updated_at = EXCLUDED.updated_at
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
        Ok(mappers::postgres_row_to_workflow(&row))
    }

    async fn fetch_workflows(&self) -> Result<Vec<WorkflowDefinition>, SendableError> {
        let rows = sqlx::query("SELECT id, name, version, enabled, input_schema, definition, created_at, updated_at FROM workflows ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(mappers::postgres_row_to_workflow).collect())
    }

    async fn fetch_workflow(
        &self,
        workflow_id: i64,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        let row = sqlx::query("SELECT id, name, version, enabled, input_schema, definition, created_at, updated_at FROM workflows WHERE id = $1")
            .bind(workflow_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::postgres_row_to_workflow(&row)))
    }

    async fn delete_workflow(&self, workflow_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(sqlx::query("DELETE FROM workflows WHERE id = $1").bind(workflow_id))
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
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT(id) DO UPDATE SET workflow_id = EXCLUDED.workflow_id, kind = EXCLUDED.kind, enabled = EXCLUDED.enabled, configuration = EXCLUDED.configuration, next_execution = EXCLUDED.next_execution, blackout_start = EXCLUDED.blackout_start, blackout_end = EXCLUDED.blackout_end, metadata = EXCLUDED.metadata, updated_at = EXCLUDED.updated_at
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
        Ok(mappers::postgres_row_to_workflow_trigger(&row))
    }

    async fn fetch_workflow_triggers(
        &self,
        workflow_id: i64,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE workflow_id = $1 ORDER BY id")
            .bind(workflow_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_workflow_trigger)
            .collect())
    }

    async fn fetch_workflow_trigger(
        &self,
        trigger_id: i64,
    ) -> Result<Option<WorkflowTrigger>, SendableError> {
        let row = sqlx::query("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE id = $1")
            .bind(trigger_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::postgres_row_to_workflow_trigger(&row)))
    }

    async fn delete_workflow_trigger(&self, trigger_id: i64) -> Result<(), SendableError> {
        self.pool
            .execute(sqlx::query("DELETE FROM workflow_triggers WHERE id = $1").bind(trigger_id))
            .await?;
        Ok(())
    }

    async fn fetch_due_workflow_triggers(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<WorkflowTrigger>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, kind, enabled, configuration, next_execution, blackout_start, blackout_end, metadata, created_at, updated_at FROM workflow_triggers WHERE enabled = TRUE AND kind = 'cron' AND (next_execution IS NULL OR next_execution <= $1) ORDER BY COALESCE(next_execution, 0), id")
            .bind(now.timestamp())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_workflow_trigger)
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
                    "UPDATE workflow_triggers SET next_execution = $1, updated_at = $2 WHERE id = $3",
                )
                .bind(next_execution.map(|dt| dt.timestamp()))
                .bind(Utc::now().timestamp())
                .bind(trigger_id),
            )
            .await?;
        Ok(())
    }

    async fn create_workflow_run(
        &self,
        workflow_id: i64,
        workflow_snapshot: WorkflowDefinition,
        parameters: Value,
        state: Value,
    ) -> Result<WorkflowRun, SendableError> {
        let row = sqlx::query(
            "INSERT INTO workflow_runs (workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at)
             VALUES ($1, $2, $3, NULL, $4, $5, $6)
             RETURNING id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name",
        )
        .bind(workflow_id)
        .bind(serde_json::to_string(&workflow_snapshot)?)
        .bind(WorkflowStatus::Queued.as_str())
        .bind(parameters.to_string())
        .bind(state.to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::postgres_row_to_workflow_run(&row))
    }

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<Option<WorkflowRun>, SendableError> {
        let row = sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs WHERE id = $1")
            .bind(workflow_run_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::postgres_row_to_workflow_run(&row)))
    }

    async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs WHERE status = $1 ORDER BY id")
            .bind(status.as_str())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_workflow_run)
            .collect())
    }

    async fn fetch_recent_workflow_runs(&self) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs ORDER BY id DESC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_workflow_run)
            .collect())
    }

    async fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        let rows = sqlx::query("SELECT id, workflow_id, workflow_snapshot, status, active_node_id, parameters, state, created_at, started_at, finished_at, message, name FROM workflow_runs WHERE workflow_id = $1 ORDER BY id DESC")
            .bind(workflow_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_workflow_run)
            .collect())
    }

    async fn set_workflow_run_name(
        &self,
        workflow_run_id: i64,
        name: Option<String>,
    ) -> Result<(), SendableError> {
        self.pool
            .execute(
                sqlx::query("UPDATE workflow_runs SET name = $1 WHERE id = $2")
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
            "UPDATE workflow_runs SET status = $1, active_node_id = COALESCE($2, active_node_id), state = COALESCE($3, state), message = COALESCE($4, message), started_at = CASE WHEN $5 = 'running' AND started_at IS NULL THEN $6 ELSE started_at END, finished_at = CASE WHEN $7 THEN $8 ELSE finished_at END WHERE id = $9",
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
             VALUES ($1, $2, $3, $4, $5, $6, $7)
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
        Ok(mappers::postgres_row_to_workflow_node_run(&row))
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
            "UPDATE workflow_node_runs SET status = $1, attempt = COALESCE($2, attempt), parameters = COALESCE($3, parameters), output_json = COALESCE($4, output_json), state = COALESCE($5, state), transition_reason = COALESCE($6, transition_reason), message = COALESCE($7, message), started_at = CASE WHEN $8 = 'running' THEN $9 WHEN $10 = 'queued' THEN NULL ELSE started_at END, finished_at = CASE WHEN $11 THEN $12 WHEN $13 = 'queued' THEN NULL ELSE finished_at END WHERE id = $14",
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
        let rows = sqlx::query("SELECT id, workflow_run_id, node_id, status, attempt, parameters, output_json, state, transition_reason, created_at, started_at, finished_at, message FROM workflow_node_runs WHERE workflow_run_id = $1 ORDER BY id")
            .bind(workflow_run_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| mappers::postgres_row_to_workflow_node_run(&row))
            .collect())
    }

    async fn fetch_workflow_node_run(
        &self,
        workflow_node_run_id: i64,
    ) -> Result<Option<WorkflowNodeRun>, SendableError> {
        let row = sqlx::query("SELECT id, workflow_run_id, node_id, status, attempt, parameters, output_json, state, transition_reason, created_at, started_at, finished_at, message FROM workflow_node_runs WHERE id = $1")
            .bind(workflow_node_run_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::postgres_row_to_workflow_node_run(&row)))
    }

    async fn append_workflow_node_run_chunk(
        &self,
        workflow_node_run_id: i64,
        chunk: &NewRunChunk,
    ) -> Result<WorkflowNodeRunChunk, SendableError> {
        let sequence: i64 = sqlx::query("SELECT COALESCE(MAX(sequence), 0) + 1 AS next_sequence FROM workflow_node_chunks WHERE workflow_node_run_id = $1")
            .bind(workflow_node_run_id)
            .fetch_one(&self.pool)
            .await?
            .get("next_sequence");
        let row = sqlx::query(
            "INSERT INTO workflow_node_chunks (workflow_node_run_id, sequence, stream, content, created_at)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, workflow_node_run_id, sequence, stream, content, created_at",
        )
        .bind(workflow_node_run_id)
        .bind(sequence)
        .bind(&chunk.stream)
        .bind(&chunk.content)
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::postgres_row_to_workflow_node_run_chunk(&row))
    }

    async fn fetch_workflow_node_run_chunks(
        &self,
        workflow_node_run_id: i64,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<WorkflowNodeRunChunk>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, workflow_node_run_id, sequence, stream, content, created_at FROM workflow_node_chunks WHERE workflow_node_run_id = $1 AND sequence > $2 ORDER BY sequence ASC LIMIT $3",
        )
        .bind(workflow_node_run_id)
        .bind(cursor.unwrap_or(0))
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_workflow_node_run_chunk)
            .collect())
    }

    async fn add_workflow_node_run_artifact(
        &self,
        workflow_node_run_id: i64,
        artifact: &NewRunArtifact,
    ) -> Result<WorkflowNodeRunArtifact, SendableError> {
        let row = sqlx::query(
            "INSERT INTO workflow_node_artifacts (workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
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
        Ok(mappers::postgres_row_to_workflow_node_run_artifact(&row))
    }

    async fn fetch_workflow_node_run_artifacts(
        &self,
        workflow_node_run_id: i64,
    ) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
        let rows = sqlx::query(
            "SELECT id, workflow_node_run_id, name, mime_type, size_bytes, uri, metadata, created_at FROM workflow_node_artifacts WHERE workflow_node_run_id = $1 ORDER BY id ASC",
        )
        .bind(workflow_node_run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_workflow_node_run_artifact)
            .collect())
    }

    async fn upsert_catalog_item(&self, item: Value) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO catalog_items (uri, item_type, name, version, document, metadata, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
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
        Ok(mappers::postgres_row_to_catalog_item(&row))
    }

    async fn fetch_catalog_items(
        &self,
        item_type: Option<String>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = if let Some(item_type) = item_type {
            sqlx::query("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items WHERE item_type = $1 ORDER BY uri")
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
            .map(mappers::postgres_row_to_catalog_item)
            .collect())
    }

    async fn fetch_catalog_item(&self, uri: String) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query("SELECT id, uri, item_type, name, version, document, metadata, created_at, updated_at FROM catalog_items WHERE uri = $1")
            .bind(uri)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::postgres_row_to_catalog_item(&row)))
    }

    async fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "INSERT INTO automation_records (record_type, workflow_run_id, external_item_id, node_id, provider, resource_type, external_id, status, title, url, body, path, prompt, approval_type, resolved_by, resolved_at, metadata, data, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
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
        Ok(mappers::postgres_row_to_automation_record(&row))
    }

    async fn update_automation_record(
        &self,
        record_type: String,
        record_id: i64,
        record: Value,
    ) -> Result<Value, SendableError> {
        let now = Utc::now().timestamp();
        let row = sqlx::query(
            "UPDATE automation_records SET workflow_run_id = $1, external_item_id = $2, node_id = $3, provider = $4, resource_type = $5, external_id = $6, status = $7, title = $8, url = $9, body = $10, path = $11, prompt = $12, approval_type = $13, resolved_by = $14, resolved_at = $15, metadata = $16, data = $17, updated_at = $18 WHERE id = $19 AND record_type = $20 RETURNING id, record_type, data, created_at, updated_at",
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
        Ok(mappers::postgres_row_to_automation_record(&row))
    }

    async fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<i64>,
        external_item_id: Option<i64>,
    ) -> Result<Vec<Value>, SendableError> {
        let rows = sqlx::query("SELECT id, record_type, data, created_at, updated_at FROM automation_records WHERE record_type = $1 ORDER BY id DESC")
            .bind(record_type)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_automation_record)
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
        let row = sqlx::query("SELECT id, record_type, data, created_at, updated_at FROM automation_records WHERE id = $1 AND record_type = $2")
            .bind(record_id)
            .bind(record_type)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::postgres_row_to_automation_record(&row)))
    }

    async fn put_idempotency_key(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> Result<Value, SendableError> {
        let row = sqlx::query(
            "INSERT INTO idempotency_keys (scope, key, result, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT(scope, key) DO UPDATE SET result = idempotency_keys.result
             RETURNING id, scope, key, result, created_at",
        )
        .bind(scope)
        .bind(key)
        .bind(result.to_string())
        .bind(Utc::now().timestamp())
        .fetch_one(&self.pool)
        .await?;
        Ok(mappers::postgres_row_to_idempotency_key(&row))
    }

    async fn fetch_idempotency_key(
        &self,
        scope: String,
        key: String,
    ) -> Result<Option<Value>, SendableError> {
        let row = sqlx::query("SELECT id, scope, key, result, created_at FROM idempotency_keys WHERE scope = $1 AND key = $2")
            .bind(scope)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| mappers::postgres_row_to_idempotency_key(&row)))
    }

    async fn create_notification(
        &self,
        notification: &NewNotification,
    ) -> Result<Notification, SendableError> {
        let row = sqlx::query(
            "INSERT INTO notifications (workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
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
        Ok(mappers::postgres_row_to_notification(&row))
    }

    async fn fetch_notifications(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> Result<Vec<Notification>, SendableError> {
        let bounded_limit = limit.clamp(1, 1000);
        let rows = if unread_only {
            sqlx::query(
                "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications WHERE read_at IS NULL ORDER BY created_at DESC LIMIT $1",
            )
            .bind(bounded_limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at FROM notifications ORDER BY created_at DESC LIMIT $1",
            )
            .bind(bounded_limit)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .iter()
            .map(mappers::postgres_row_to_notification)
            .collect())
    }

    async fn mark_notification_read(
        &self,
        notification_id: i64,
    ) -> Result<Option<Notification>, SendableError> {
        let row = sqlx::query(
            "UPDATE notifications SET read_at = COALESCE(read_at, $1) WHERE id = $2 RETURNING id, workflow_run_id, workflow_node_id, channel, severity, title, body, target, metadata, read_at, created_at",
        )
        .bind(Utc::now().timestamp())
        .bind(notification_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| mappers::postgres_row_to_notification(&row)))
    }

    async fn mark_all_notifications_read(&self) -> Result<u64, SendableError> {
        let result = sqlx::query("UPDATE notifications SET read_at = $1 WHERE read_at IS NULL")
            .bind(Utc::now().timestamp())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}
