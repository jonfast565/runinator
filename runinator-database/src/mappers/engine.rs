use super::*;

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

row_mapper!(row_to_action_dispatch(row) -> Result<ActionDispatchRecord, SendableError> {
    action_dispatch_from_row!(row)
});

macro_rules! orchestration_event_from_row {
    ($row:expr) => {{
        Ok(OrchestrationEvent {
            event_id: $row.get::<Uuid, _>("event_id"),
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

row_mapper!(row_to_orchestration_event(row) -> Result<OrchestrationEvent, SendableError> {
    orchestration_event_from_row!(row)
});

macro_rules! ready_node_from_row {
    ($row:expr) => {{
        Ok(ReadyNodeRecord {
            id: $row.get("id"),
            source_event_id: $row.get::<Uuid, _>("source_event_id"),
            workflow_run_id: $row.get("workflow_run_id"),
            node_id: $row.get("node_id"),
            cursor_id: $row.try_get("cursor_id").ok().flatten(),
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

row_mapper!(row_to_ready_node(row) -> Result<ReadyNodeRecord, SendableError> {
    ready_node_from_row!(row)
});
