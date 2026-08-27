use super::*;
use runinator_models::workspaces::{WorkspaceLease, WorkspaceStatus};

fallible_row_mapper!(row_to_workspace_lease(row) -> WorkspaceLease {
    let raw_status = row.get::<String, _>("status");
    let status = WorkspaceStatus::try_from(raw_status.as_str())
        .map_err(|error| Box::new(std::io::Error::other(error)) as SendableError)?;
    Ok(WorkspaceLease {
        id: row.get("id"),
        admission_id: row.get("admission_id"),
        generation: row.get("generation"),
        scope: row.get("scope"),
        attempt: row.get("attempt"),
        worker_instance_id: row.get("worker_instance_id"),
        worker_replica_id: row.get("worker_replica_id"),
        local_key: row.get("local_key"),
        requirements: parse_json(row.get("requirements")),
        status,
        version: row.get("version"),
        leased_until: DateTime::<Utc>::from_timestamp(row.get("leased_until"), 0).unwrap_or_else(Utc::now),
        unavailable_since: row.get::<Option<i64>, _>("unavailable_since")
            .and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
        evidence: parse_json(row.get("evidence")),
        created_at: DateTime::<Utc>::from_timestamp(row.get("created_at"), 0).unwrap_or_else(Utc::now),
        updated_at: DateTime::<Utc>::from_timestamp(row.get("updated_at"), 0).unwrap_or_else(Utc::now),
    })
});
