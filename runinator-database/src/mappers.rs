use chrono::{DateTime, Utc};
use runinator_models::{
    core::ScheduledTask,
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStepRun},
};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow, sqlite::SqliteRow};

fn parse_json(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(Value::Null)
}

macro_rules! scheduled_task_from_row {
    ($row:expr) => {{
        let next_execution = $row
            .get::<Option<i64>, _>("next_execution")
            .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

        let blackout_start = $row
            .get::<Option<i64>, _>("blackout_start")
            .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

        let blackout_end = $row
            .get::<Option<i64>, _>("blackout_end")
            .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));

        ScheduledTask {
            id: $row.get::<Option<i64>, _>("id"),
            name: $row.get::<String, _>("name"),
            cron_schedule: $row.get::<String, _>("cron_schedule"),
            action_name: $row.get::<String, _>("action_name"),
            action_function: $row.get::<String, _>("action_function"),
            action_configuration: $row.get::<String, _>("action_configuration"),
            timeout: $row.get::<i64, _>("timeout"),
            next_execution,
            enabled: $row.get::<bool, _>("enabled"),
            immediate: $row.get::<bool, _>("immediate"),
            blackout_start,
            blackout_end,
            input_schema: parse_json($row.get::<String, _>("input_schema")),
            default_parameters: parse_json($row.get::<String, _>("default_parameters")),
            output_schema: $row
                .get::<Option<String>, _>("output_schema")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            mcp_enabled: $row.get::<bool, _>("mcp_enabled"),
            metadata: parse_json($row.get::<String, _>("metadata")),
            tags: serde_json::from_str(&$row.get::<String, _>("tags")).unwrap_or_default(),
        }
    }};
}

pub fn sqlite_row_to_scheduled_task(row: &SqliteRow) -> ScheduledTask {
    scheduled_task_from_row!(row)
}

pub fn postgres_row_to_scheduled_task(row: &PgRow) -> ScheduledTask {
    scheduled_task_from_row!(row)
}

macro_rules! run_summary_from_row {
    ($row:expr) => {{
        RunSummary {
            id: $row.get("id"),
            task_id: $row.get("task_id"),
            status: RunStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(RunStatus::Failed),
            parameters: parse_json($row.get::<String, _>("parameters")),
            output_json: $row
                .get::<Option<String>, _>("output_json")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            message: $row.get("message"),
            trigger: $row.get("trigger"),
            started_at: $row
                .get::<Option<i64>, _>("started_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            finished_at: $row
                .get::<Option<i64>, _>("finished_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            workflow_run_id: $row.get("workflow_run_id"),
            workflow_step_id: $row.get("workflow_step_id"),
        }
    }};
}

pub fn sqlite_row_to_run_summary(row: &SqliteRow) -> RunSummary {
    run_summary_from_row!(row)
}

pub fn postgres_row_to_run_summary(row: &PgRow) -> RunSummary {
    run_summary_from_row!(row)
}

macro_rules! run_chunk_from_row {
    ($row:expr) => {{
        RunChunk {
            id: $row.get("id"),
            run_id: $row.get("run_id"),
            sequence: $row.get("sequence"),
            stream: $row.get("stream"),
            content: $row.get("content"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

pub fn sqlite_row_to_run_chunk(row: &SqliteRow) -> RunChunk {
    run_chunk_from_row!(row)
}

pub fn postgres_row_to_run_chunk(row: &PgRow) -> RunChunk {
    run_chunk_from_row!(row)
}

macro_rules! run_artifact_from_row {
    ($row:expr) => {{
        RunArtifact {
            id: $row.get("id"),
            run_id: $row.get("run_id"),
            name: $row.get("name"),
            mime_type: $row.get("mime_type"),
            size_bytes: $row.get("size_bytes"),
            uri: $row.get("uri"),
            metadata: parse_json($row.get::<String, _>("metadata")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

pub fn sqlite_row_to_run_artifact(row: &SqliteRow) -> RunArtifact {
    run_artifact_from_row!(row)
}

pub fn postgres_row_to_run_artifact(row: &PgRow) -> RunArtifact {
    run_artifact_from_row!(row)
}

macro_rules! workflow_from_row {
    ($row:expr) => {{
        WorkflowDefinition {
            id: $row.get("id"),
            name: $row.get("name"),
            version: $row.get("version"),
            enabled: $row.get("enabled"),
            input_schema: parse_json($row.get::<String, _>("input_schema")),
            definition: parse_json($row.get::<String, _>("definition")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0),
        }
    }};
}

pub fn sqlite_row_to_workflow(row: &SqliteRow) -> WorkflowDefinition {
    workflow_from_row!(row)
}

pub fn postgres_row_to_workflow(row: &PgRow) -> WorkflowDefinition {
    workflow_from_row!(row)
}

macro_rules! workflow_run_from_row {
    ($row:expr) => {{
        WorkflowRun {
            id: $row.get("id"),
            workflow_id: $row.get("workflow_id"),
            status: RunStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(RunStatus::Failed),
            parameters: parse_json($row.get::<String, _>("parameters")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            started_at: $row
                .get::<Option<i64>, _>("started_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            finished_at: $row
                .get::<Option<i64>, _>("finished_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            message: $row.get("message"),
        }
    }};
}

pub fn sqlite_row_to_workflow_run(row: &SqliteRow) -> WorkflowRun {
    workflow_run_from_row!(row)
}

pub fn postgres_row_to_workflow_run(row: &PgRow) -> WorkflowRun {
    workflow_run_from_row!(row)
}

macro_rules! workflow_step_run_from_row {
    ($row:expr) => {{
        WorkflowStepRun {
            id: $row.get("id"),
            workflow_run_id: $row.get("workflow_run_id"),
            step_id: $row.get("step_id"),
            task_run_id: $row.get("task_run_id"),
            status: RunStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(RunStatus::Failed),
            attempt: $row.get("attempt"),
            parameters: parse_json($row.get::<String, _>("parameters")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            started_at: $row
                .get::<Option<i64>, _>("started_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            finished_at: $row
                .get::<Option<i64>, _>("finished_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            message: $row.get("message"),
        }
    }};
}

pub fn sqlite_row_to_workflow_step_run(row: &SqliteRow) -> WorkflowStepRun {
    workflow_step_run_from_row!(row)
}

pub fn postgres_row_to_workflow_step_run(row: &PgRow) -> WorkflowStepRun {
    workflow_step_run_from_row!(row)
}
