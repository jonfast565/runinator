use super::*;

macro_rules! notification_from_row {
    ($row:expr) => {{
        Notification {
            id: $row.get::<Uuid, _>("id"),
            workflow_run_id: $row.get::<Option<Uuid>, _>("workflow_run_id"),
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

row_mapper!(row_to_notification(row) -> Notification { notification_from_row!(row) });

macro_rules! notification_policy_from_row {
    ($row:expr) => {{
        NotificationPolicy {
            id: $row.get::<Uuid, _>("id"),
            workflow_id: $row.get::<Option<Uuid>, _>("workflow_id"),
            name: $row.get::<String, _>("name"),
            // an unrecognized event/severity/channel means a newer writer or hand-edited row; fall
            // back to the safest interpretation rather than dropping the policy entirely.
            event: NotificationEvent::try_from($row.get::<String, _>("event").as_str())
                .unwrap_or(NotificationEvent::RunFailed),
            severity: NotificationSeverity::try_from($row.get::<String, _>("severity").as_str())
                .unwrap_or(NotificationSeverity::Warning),
            channel: NotificationChannel::try_from($row.get::<String, _>("channel").as_str())
                .unwrap_or(NotificationChannel::InApp),
            target: $row.get::<Option<String>, _>("target"),
            threshold_seconds: $row.get::<Option<i64>, _>("threshold_seconds"),
            enabled: $row.get("enabled"),
            managed_by: $row.get::<Option<String>, _>("managed_by"),
            configuration: parse_json($row.get::<String, _>("configuration")),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_notification_policy(row) -> NotificationPolicy {
    notification_policy_from_row!(row)
});

macro_rules! notification_delivery_from_row {
    ($row:expr) => {{
        NotificationDelivery {
            id: $row.get::<Uuid, _>("id"),
            notification_id: $row.get::<Uuid, _>("notification_id"),
            policy_id: $row.get::<Option<Uuid>, _>("policy_id"),
            channel: NotificationChannel::try_from($row.get::<String, _>("channel").as_str())
                .unwrap_or(NotificationChannel::InApp),
            target: $row.get::<Option<String>, _>("target"),
            status: NotificationDeliveryStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(NotificationDeliveryStatus::Pending),
            attempts: $row.get::<i64, _>("attempts"),
            last_error: $row.get::<Option<String>, _>("last_error"),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_notification_delivery(row) -> NotificationDelivery {
    notification_delivery_from_row!(row)
});

fallible_row_mapper!(row_to_notification_effect_dispatch(row) -> runinator_comm::NotificationEffectDispatchRecord {
    let command: runinator_comm::EffectCommand = serde_json::from_str(&row.get::<String, _>("command_json"))
        .map_err(|error| crate::errors::WORKFLOW_VM_CORRUPT_STATE.error(error))?;
    command.ensure_supported().map_err(|error| crate::errors::WORKFLOW_VM_CORRUPT_STATE.error(error))?;
    Ok(runinator_comm::NotificationEffectDispatchRecord {
        delivery_id: row.get("id"),
        dedupe_key: row.get("dedupe_key"),
        command,
        attempts: row.get("attempts"),
        created_at: DateTime::<Utc>::from_timestamp(row.get("created_at"), 0).unwrap_or_else(Utc::now),
        updated_at: DateTime::<Utc>::from_timestamp(row.get("updated_at"), 0).unwrap_or_else(Utc::now),
        published_at: row.get::<Option<i64>, _>("published_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
        last_error: row.get("last_error"),
        claimed_by: row.get("claimed_by"),
        claimed_until: row.get::<Option<i64>, _>("claimed_until").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
    })
});

macro_rules! freeze_window_from_row {
    ($row:expr) => {{
        FreezeWindow {
            id: $row.get::<Uuid, _>("id"),
            org_id: $row.get::<Option<Uuid>, _>("org_id"),
            workflow_id: $row.get::<Option<Uuid>, _>("workflow_id"),
            name: $row.get::<String, _>("name"),
            reason: $row.get::<Option<String>, _>("reason"),
            starts_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("starts_at"), 0)
                .unwrap_or_else(Utc::now),
            ends_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("ends_at"), 0)
                .unwrap_or_else(Utc::now),
            schedule: $row
                .get::<Option<String>, _>("schedule")
                .and_then(|raw| serde_json::from_str(&raw).ok()),
            enabled: $row.get("enabled"),
            created_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("created_at"), 0)
                .unwrap_or_else(Utc::now),
            updated_at: DateTime::<Utc>::from_timestamp($row.get::<i64, _>("updated_at"), 0)
                .unwrap_or_else(Utc::now),
        }
    }};
}

row_mapper!(row_to_freeze_window(row) -> FreezeWindow { freeze_window_from_row!(row) });
