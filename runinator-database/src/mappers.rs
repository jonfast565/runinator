use chrono::{DateTime, Utc};
use runinator_models::{
    core::ScheduledTask,
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
    workflows::{WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
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
            workflow_node_id: $row.get("workflow_node_id"),
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
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
            active_node_id: $row.get("active_node_id"),
            parameters: parse_json($row.get::<String, _>("parameters")),
            state: parse_json($row.get::<String, _>("state")),
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

macro_rules! workflow_node_run_from_row {
    ($row:expr) => {{
        WorkflowNodeRun {
            id: $row.get("id"),
            workflow_run_id: $row.get("workflow_run_id"),
            node_id: $row.get("node_id"),
            task_run_id: $row.get("task_run_id"),
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
            attempt: $row.get("attempt"),
            parameters: parse_json($row.get::<String, _>("parameters")),
            output_json: $row
                .get::<Option<String>, _>("output_json")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            state: parse_json($row.get::<String, _>("state")),
            transition_reason: $row.get("transition_reason"),
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

pub fn sqlite_row_to_workflow_node_run(row: &SqliteRow) -> WorkflowNodeRun {
    workflow_node_run_from_row!(row)
}

pub fn postgres_row_to_workflow_node_run(row: &PgRow) -> WorkflowNodeRun {
    workflow_node_run_from_row!(row)
}

macro_rules! catalog_item_from_row {
    ($row:expr) => {{
        serde_json::json!({
            "id": $row.get::<i64, _>("id"),
            "uri": $row.get::<String, _>("uri"),
            "item_type": $row.get::<String, _>("item_type"),
            "name": $row.get::<String, _>("name"),
            "version": $row.get::<String, _>("version"),
            "document": parse_json($row.get::<String, _>("document")),
            "metadata": parse_json($row.get::<String, _>("metadata")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0).unwrap_or_else(Utc::now),
            "updated_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0).unwrap_or_else(Utc::now),
        })
    }};
}

pub fn sqlite_row_to_catalog_item(row: &SqliteRow) -> Value {
    catalog_item_from_row!(row)
}

pub fn postgres_row_to_catalog_item(row: &PgRow) -> Value {
    catalog_item_from_row!(row)
}

macro_rules! automation_record_from_row {
    ($row:expr) => {{
        let mut data = parse_json($row.get::<String, _>("data"));
        if !data.is_object() {
            data = Value::Object(Default::default());
        }
        if let Some(object) = data.as_object_mut() {
            object.insert("id".into(), Value::from($row.get::<i64, _>("id")));
            object.insert(
                "record_type".into(),
                Value::from($row.get::<String, _>("record_type")),
            );
            object.insert(
                "created_at".into(),
                Value::from(
                    DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                        .unwrap_or_else(Utc::now)
                        .to_rfc3339(),
                ),
            );
            object.insert(
                "updated_at".into(),
                Value::from(
                    DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0)
                        .unwrap_or_else(Utc::now)
                        .to_rfc3339(),
                ),
            );
        }
        data
    }};
}

pub fn sqlite_row_to_automation_record(row: &SqliteRow) -> Value {
    automation_record_from_row!(row)
}

pub fn postgres_row_to_automation_record(row: &PgRow) -> Value {
    automation_record_from_row!(row)
}

macro_rules! idempotency_key_from_row {
    ($row:expr) => {{
        serde_json::json!({
            "id": $row.get::<i64, _>("id"),
            "scope": $row.get::<String, _>("scope"),
            "key": $row.get::<String, _>("key"),
            "result": parse_json($row.get::<String, _>("result")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0).unwrap_or_else(Utc::now),
        })
    }};
}

pub fn sqlite_row_to_idempotency_key(row: &SqliteRow) -> Value {
    idempotency_key_from_row!(row)
}

pub fn postgres_row_to_idempotency_key(row: &PgRow) -> Value {
    idempotency_key_from_row!(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_json() {
        assert_eq!(parse_json("{\"a\":1}".to_string()), json!({"a":1}));
        assert_eq!(parse_json("invalid".to_string()), Value::Null);
    }
}
