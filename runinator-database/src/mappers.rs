use chrono::{DateTime, Utc};
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use runinator_models::value::Value;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    notifications::Notification,
    orchestration::{OrchestrationEvent, ReadyNodeRecord},
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
    settings::{SettingKind, SettingRecord},
    types::RuninatorType,
    workflows::{
        WorkflowDefinition, WorkflowGraph, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowStatus, WorkflowTrigger, WorkflowTriggerKind,
    },
};
use sqlx::{Row, postgres::PgRow, sqlite::SqliteRow};

fn parse_json(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(Value::Null)
}

fn parse_type(raw: String) -> RuninatorType {
    let value = parse_json(raw);
    serde_json::from_value(value.clone().into())
        .unwrap_or_else(|_| RuninatorType::from_json_schema(&value))
}

macro_rules! run_summary_from_row {
    ($row:expr) => {{
        RunSummary {
            id: $row.get("id"),
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

macro_rules! setting_from_row {
    ($row:expr) => {{
        SettingRecord {
            kind: SettingKind::from_str_lossy(&$row.get::<String, _>("kind")),
            scope: $row.get("scope"),
            name: $row.get("name"),
            value: $row.get("value"),
            updated_at: $row.get("updated_at"),
        }
    }};
}

pub fn sqlite_row_to_setting(row: &SqliteRow) -> SettingRecord {
    setting_from_row!(row)
}

pub fn postgres_row_to_setting(row: &PgRow) -> SettingRecord {
    setting_from_row!(row)
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
            input_type: parse_type($row.get::<String, _>("input_schema")),
            definition: WorkflowGraph::from_value(parse_json($row.get::<String, _>("definition")))
                .unwrap_or_default(),
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

macro_rules! workflow_trigger_from_row {
    ($row:expr) => {{
        WorkflowTrigger {
            id: $row.get("id"),
            workflow_id: $row.get("workflow_id"),
            kind: WorkflowTriggerKind::try_from($row.get::<String, _>("kind").as_str())
                .unwrap_or(WorkflowTriggerKind::Manual),
            enabled: $row.get("enabled"),
            configuration: parse_json($row.get::<String, _>("configuration")),
            next_execution: $row
                .get::<Option<i64>, _>("next_execution")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            blackout_start: $row
                .get::<Option<i64>, _>("blackout_start")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            blackout_end: $row
                .get::<Option<i64>, _>("blackout_end")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            metadata: parse_json($row.get::<String, _>("metadata")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0),
        }
    }};
}

pub fn sqlite_row_to_workflow_trigger(row: &SqliteRow) -> WorkflowTrigger {
    workflow_trigger_from_row!(row)
}

pub fn postgres_row_to_workflow_trigger(row: &PgRow) -> WorkflowTrigger {
    workflow_trigger_from_row!(row)
}

macro_rules! workflow_run_from_row {
    ($row:expr) => {{
        WorkflowRun {
            id: $row.get("id"),
            workflow_id: $row.get("workflow_id"),
            workflow_snapshot: $row
                .get::<Option<String>, _>("workflow_snapshot")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
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
            name: $row.get("name"),
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

macro_rules! workflow_node_run_chunk_from_row {
    ($row:expr) => {{
        WorkflowNodeRunChunk {
            id: $row.get("id"),
            workflow_node_run_id: $row.get("workflow_node_run_id"),
            sequence: $row.get("sequence"),
            stream: $row.get("stream"),
            content: $row.get("content"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

pub fn sqlite_row_to_workflow_node_run_chunk(row: &SqliteRow) -> WorkflowNodeRunChunk {
    workflow_node_run_chunk_from_row!(row)
}

pub fn postgres_row_to_workflow_node_run_chunk(row: &PgRow) -> WorkflowNodeRunChunk {
    workflow_node_run_chunk_from_row!(row)
}

macro_rules! workflow_node_run_artifact_from_row {
    ($row:expr) => {{
        WorkflowNodeRunArtifact {
            id: $row.get("id"),
            workflow_node_run_id: $row.get("workflow_node_run_id"),
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

pub fn sqlite_row_to_workflow_node_run_artifact(row: &SqliteRow) -> WorkflowNodeRunArtifact {
    workflow_node_run_artifact_from_row!(row)
}

pub fn postgres_row_to_workflow_node_run_artifact(row: &PgRow) -> WorkflowNodeRunArtifact {
    workflow_node_run_artifact_from_row!(row)
}

macro_rules! catalog_item_from_row {
    ($row:expr) => {{
        runinator_models::json!({
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
        runinator_models::json!({
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

macro_rules! action_dispatch_from_row {
    ($row:expr) => {{
        let raw = $row.get::<String, _>("command_json");
        Ok(ActionDispatchRecord {
            id: $row.get("id"),
            dedupe_key: $row.get("dedupe_key"),
            command: parse_action_command(raw)?,
            attempts: $row.get("attempts"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
            published_at: $row
                .get::<Option<i64>, _>("published_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            last_error: $row.get("last_error"),
            claimed_by: $row
                .try_get::<Option<String>, _>("claimed_by")
                .ok()
                .flatten(),
            claimed_until: $row
                .try_get::<Option<i64>, _>("claimed_until")
                .ok()
                .flatten()
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        })
    }};
}

fn parse_action_command(raw: String) -> Result<ActionCommand, SendableError> {
    serde_json::from_str::<ActionCommand>(&raw).map_err(|err| {
        Box::new(RuntimeError::new(
            "database.action_dispatch.invalid_command_json".into(),
            format!("Stored action dispatch command is invalid JSON: {err}"),
        )) as SendableError
    })
}

pub fn sqlite_row_to_action_dispatch(
    row: &SqliteRow,
) -> Result<ActionDispatchRecord, SendableError> {
    action_dispatch_from_row!(row)
}

pub fn postgres_row_to_action_dispatch(row: &PgRow) -> Result<ActionDispatchRecord, SendableError> {
    action_dispatch_from_row!(row)
}

macro_rules! orchestration_event_from_row {
    ($row:expr) => {{
        let event_id = $row.get::<String, _>("event_id").parse().map_err(|err| {
            Box::new(RuntimeError::new(
                "database.orchestration_event.invalid_event_id".into(),
                format!("Stored orchestration event id is invalid: {err}"),
            )) as SendableError
        })?;
        Ok(OrchestrationEvent {
            event_id,
            workflow_run_id: $row.get("workflow_run_id"),
            workflow_node_run_id: $row.get("workflow_node_run_id"),
            node_id: $row.get("node_id"),
            event_type: $row.get("event_type"),
            payload: parse_json($row.get::<String, _>("payload")),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
        })
    }};
}

pub fn sqlite_row_to_orchestration_event(
    row: &SqliteRow,
) -> Result<OrchestrationEvent, SendableError> {
    orchestration_event_from_row!(row)
}

pub fn postgres_row_to_orchestration_event(
    row: &PgRow,
) -> Result<OrchestrationEvent, SendableError> {
    orchestration_event_from_row!(row)
}

macro_rules! ready_node_from_row {
    ($row:expr) => {{
        let source_event_id = $row
            .get::<String, _>("source_event_id")
            .parse()
            .map_err(|err| {
                Box::new(RuntimeError::new(
                    "database.ready_node.invalid_source_event_id".into(),
                    format!("Stored ready-node source event id is invalid: {err}"),
                )) as SendableError
            })?;
        Ok(ReadyNodeRecord {
            id: $row.get("id"),
            source_event_id,
            workflow_run_id: $row.get("workflow_run_id"),
            node_id: $row.get("node_id"),
            status: WorkflowStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(WorkflowStatus::Failed),
            ready_at: DateTime::<Utc>::from_timestamp($row.get("ready_at"), 0)
                .unwrap_or_else(Utc::now),
            attempts: $row.get("attempts"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
            claimed_by: $row.get("claimed_by"),
            claimed_until: $row
                .get::<Option<i64>, _>("claimed_until")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            completed_at: $row
                .get::<Option<i64>, _>("completed_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        })
    }};
}

pub fn sqlite_row_to_ready_node(row: &SqliteRow) -> Result<ReadyNodeRecord, SendableError> {
    ready_node_from_row!(row)
}

pub fn postgres_row_to_ready_node(row: &PgRow) -> Result<ReadyNodeRecord, SendableError> {
    ready_node_from_row!(row)
}

macro_rules! notification_from_row {
    ($row:expr) => {{
        Notification {
            id: $row.get::<i64, _>("id"),
            workflow_run_id: $row.get::<Option<i64>, _>("workflow_run_id"),
            workflow_node_id: $row.get::<Option<String>, _>("workflow_node_id"),
            channel: $row.get::<String, _>("channel"),
            severity: $row.get::<String, _>("severity"),
            title: $row.get::<String, _>("title"),
            body: $row.get::<Option<String>, _>("body"),
            target: $row.get::<Option<String>, _>("target"),
            metadata: parse_json($row.get::<String, _>("metadata")),
            read_at: $row
                .get::<Option<i64>, _>("read_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

pub fn sqlite_row_to_notification(row: &SqliteRow) -> Notification {
    notification_from_row!(row)
}

pub fn postgres_row_to_notification(row: &PgRow) -> Notification {
    notification_from_row!(row)
}

#[cfg(test)]
#[path = "mappers_tests.rs"]
mod tests;
