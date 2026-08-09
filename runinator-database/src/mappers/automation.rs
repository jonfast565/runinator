use super::*;

macro_rules! catalog_item_from_row {
    ($row:expr) => {{
        runinator_models::json!({
            "id": $row.get::<Uuid, _>("id"),
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

row_mapper!(row_to_catalog_item(row) -> Value { catalog_item_from_row!(row) });

macro_rules! automation_record_from_row {
    ($row:expr) => {{
        let mut data = parse_json($row.get::<String, _>("data"));
        if !data.is_object() {
            data = Value::Object(Default::default());
        }
        if let Some(object) = data.as_object_mut() {
            object.insert(
                "id".into(),
                Value::from($row.get::<Uuid, _>("id").to_string()),
            );
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

row_mapper!(row_to_automation_record(row) -> Value { automation_record_from_row!(row) });

macro_rules! gate_from_row {
    ($row:expr) => {{
        let mut data = parse_json($row.get::<String, _>("data"));
        if !data.is_object() {
            data = Value::Object(Default::default());
        }
        if let Some(object) = data.as_object_mut() {
            object.insert(
                "id".into(),
                Value::from($row.get::<Uuid, _>("id").to_string()),
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

row_mapper!(row_to_gate(row) -> Value { gate_from_row!(row) });

macro_rules! dead_letter_from_row {
    ($row:expr) => {{
        runinator_models::json!({
            "id": $row.get::<Uuid, _>("id"),
            "channel": $row.get::<String, _>("channel"),
            "event_id": $row.get::<Option<Uuid>, _>("event_id"),
            "dedupe_key": $row.get::<Option<String>, _>("dedupe_key"),
            "attempts": $row.get::<i64, _>("attempts"),
            "error": $row.get::<String, _>("error"),
            "payload": parse_json($row.get::<String, _>("payload")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_dead_letter(row) -> Value { dead_letter_from_row!(row) });

macro_rules! audit_log_from_row {
    ($row:expr) => {{
        runinator_models::json!({
            "id": $row.get::<Uuid, _>("id"),
            "actor_id": $row.get::<Option<Uuid>, _>("actor_id"),
            "actor_kind": $row.get::<String, _>("actor_kind"),
            "action": $row.get::<String, _>("action"),
            "resource_type": $row.get::<Option<String>, _>("resource_type"),
            "resource_id": $row.get::<Option<Uuid>, _>("resource_id"),
            "outcome": $row.get::<String, _>("outcome"),
            "detail": $row.get::<Option<String>, _>("detail"),
            "metadata": parse_json($row.get::<String, _>("metadata")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_audit_log(row) -> Value { audit_log_from_row!(row) });

macro_rules! idempotency_key_from_row {
    ($row:expr) => {{
        runinator_models::json!({
            "id": $row.get::<Uuid, _>("id"),
            "scope": $row.get::<String, _>("scope"),
            "key": $row.get::<String, _>("key"),
            "result": parse_json($row.get::<String, _>("result")),
            "created_at": DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0).unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_idempotency_key(row) -> Value { idempotency_key_from_row!(row) });
