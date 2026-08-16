//! row mappers for the wdl console.

use super::*;

macro_rules! console_session_from_row {
    ($row:expr) => {{
        ConsoleSession {
            id: $row.get("id"),
            org_id: $row.get("org_id"),
            name: $row.get("name"),
            created_by: $row.get("created_by"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_console_session(row) -> ConsoleSession { console_session_from_row!(row) });

macro_rules! console_cell_from_row {
    ($row:expr) => {{
        ConsoleCell {
            id: $row.get("id"),
            session_id: $row.get("session_id"),
            position: $row.get("position"),
            label: $row.get("label"),
            source: $row.get("source"),
            // an unrecognized kind or status reads as absent rather than failing the row: a cell is
            // still worth showing when a newer replica wrote a value this build has not heard of.
            kind: $row
                .get::<Option<String>, _>("kind")
                .and_then(|raw| ConsoleCellKind::try_from(raw.as_str()).ok()),
            status: ConsoleCellStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(ConsoleCellStatus::Idle),
            result: $row
                .get::<Option<String>, _>("result")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            error: $row.get("error"),
            workflow_run_id: $row.get("workflow_run_id"),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_console_cell(row) -> ConsoleCell { console_cell_from_row!(row) });

macro_rules! console_binding_from_row {
    ($row:expr) => {{
        ConsoleBinding {
            id: $row.get("id"),
            session_id: $row.get("session_id"),
            name: $row.get("name"),
            cell_id: $row.get("cell_id"),
            value: serde_json::from_str($row.get::<String, _>("value").as_str())
                .unwrap_or(Value::Null),
            created_at: DateTime::<Utc>::from_timestamp($row.get("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_console_binding(row) -> ConsoleBinding { console_binding_from_row!(row) });
