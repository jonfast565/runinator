use super::*;

row_mapper!(row_to_execution_profile(row) -> ExecutionProfile {
    ExecutionProfile {
        id: row.get("id"),
        org_id: row.get("org_id"),
        name: row.get("name"),
        description: row.get("description"),
        credential_scopes: serde_json::from_str::<Vec<String>>(&row.get::<String, _>("credential_scopes")).unwrap_or_default(),
        collection: serde_json::from_str::<ExecutionProfileCollectionSpec>(&row.get::<String, _>("collection_json")).unwrap_or_default(),
        exposure: serde_json::from_str::<ExecutionProfileExposureSpec>(&row.get::<String, _>("exposure_json")).unwrap_or_default(),
        config_version: row.get("config_version"),
        config_digest: row.get("config_digest"),
        enabled: row.get("enabled"),
        current_revision: row.get("current_revision"),
        current_digest: row.get("current_digest"),
        current_publisher_id: row.get("current_publisher_id"),
        published_at: row.get::<Option<i64>, _>("published_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
        expires_at: row.get::<Option<i64>, _>("expires_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
        refresh_requested_at: row.get::<Option<i64>, _>("refresh_requested_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
        health: ExecutionProfileHealth::parse(&row.get::<String, _>("health")),
        last_error: row.get("last_error"),
        created_at: DateTime::<Utc>::from_timestamp(row.get("created_at"), 0).unwrap_or_else(Utc::now),
        updated_at: DateTime::<Utc>::from_timestamp(row.get("updated_at"), 0).unwrap_or_else(Utc::now),
    }
});

row_mapper!(row_to_execution_profile_revision(row) -> ExecutionProfileRevision {
    ExecutionProfileRevision {
        profile_id: row.get("profile_id"),
        revision: row.get("revision"),
        digest: row.get("digest"),
        size_bytes: row.get("size_bytes"),
        publisher_id: row.get("publisher_id"),
        expires_at: row.get::<Option<i64>, _>("expires_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
        created_at: DateTime::<Utc>::from_timestamp(row.get("created_at"), 0).unwrap_or_else(Utc::now),
        uri: row.get("uri"),
    }
});
