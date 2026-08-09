use super::*;

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

row_mapper!(row_to_run_summary(row) -> RunSummary { run_summary_from_row!(row) });

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

row_mapper!(row_to_setting(row) -> SettingRecord { setting_from_row!(row) });
