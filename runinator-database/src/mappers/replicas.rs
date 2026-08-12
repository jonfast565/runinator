use super::*;

row_mapper!(row_to_agent_directive(row) -> Result<AgentDirectiveRecord, SendableError> {
    let kind = serde_json::from_str::<AgentDirectiveKind>(&row.get::<String, _>("kind_json"))
        .map_err(|err| Box::new(err) as SendableError)?;
    let state = match row.get::<String, _>("state").as_str() {
        "pending" => AgentDirectiveState::Pending,
        "published" => AgentDirectiveState::Published,
        "accepted" => AgentDirectiveState::Accepted,
        "completed" => AgentDirectiveState::Completed,
        "failed" => AgentDirectiveState::Failed,
        "unsupported" => AgentDirectiveState::Unsupported,
        "expired" => AgentDirectiveState::Expired,
        _ => AgentDirectiveState::Failed,
    };
    Ok(AgentDirectiveRecord {
        directive_id: row.get("directive_id"),
        replica_id: row.get("replica_id"),
        kind,
        state,
        issued_at: DateTime::<Utc>::from_timestamp(row.get("issued_at"), 0).unwrap_or_else(Utc::now),
        expires_at: DateTime::<Utc>::from_timestamp(row.get("expires_at"), 0).unwrap_or_else(Utc::now),
        published_at: row.get::<Option<i64>, _>("published_at").and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        completed_at: row.get::<Option<i64>, _>("completed_at").and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        payload: parse_json(row.get("payload_json")),
        message: row.get("message"),
        attempts: row.get("attempts"),
        claimed_at: row.get::<Option<i64>, _>("claimed_at").and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        claimed_by_runtime_id: row.get("claimed_by_runtime_id"),
    })
});

macro_rules! replica_from_row {
    ($row:expr) => {{
        Ok(ReplicaRecord {
            replica_id: $row.get("replica_id"),
            replica_type: ReplicaKind::try_from($row.get::<String, _>("replica_type").as_str())
                .unwrap_or(ReplicaKind::Worker),
            instance_id: $row.get("instance_id"),
            runtime_id: $row.get("runtime_id"),
            status: ReplicaStatus::try_from($row.get::<String, _>("status").as_str())
                .unwrap_or(ReplicaStatus::Offline),
            display_name: $row.get("display_name"),
            host: $row.get("host"),
            port: $row
                .get::<Option<i64>, _>("port")
                .and_then(|value| u16::try_from(value).ok()),
            base_path: $row.get("base_path"),
            observed_ip: $row.get("observed_ip"),
            version: $row.get("version"),
            attributes: parse_json($row.get::<String, _>("attributes")),
            first_seen_at: DateTime::<Utc>::from_timestamp($row.get("first_seen_at"), 0)
                .unwrap_or_else(Utc::now),
            last_heartbeat_at: DateTime::<Utc>::from_timestamp($row.get("last_heartbeat_at"), 0)
                .unwrap_or_else(Utc::now),
            last_seen_at: DateTime::<Utc>::from_timestamp($row.get("last_seen_at"), 0)
                .unwrap_or_else(Utc::now),
            offline_at: $row
                .get::<Option<i64>, _>("offline_at")
                .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
            registered_by_principal_id: $row.get("registered_by_principal_id"),
            registered_by_kind: $row.get("registered_by_kind"),
            registered_by_org_id: $row.get("registered_by_org_id"),
        })
    }};
}

row_mapper!(row_to_replica(row) -> Result<ReplicaRecord, SendableError> {
    replica_from_row!(row)
});

row_mapper!(row_to_replica_sample(row) -> ReplicaSample {
    // the sample's numeric fields ride in a json `data` column (avoiding typed float columns), with
    // replica_id/sampled_at duplicated as typed columns for indexing and pruning.
    serde_json::from_str::<ReplicaSample>(&row.get::<String, _>("data")).unwrap_or_else(|_| {
        ReplicaSample {
            replica_id: row.get("replica_id"),
            sampled_at: DateTime::<Utc>::from_timestamp(row.get("sampled_at"), 0)
                .unwrap_or_else(Utc::now),
            cpu_percent: 0.0,
            mem_percent: 0.0,
            mem_used_bytes: 0,
            mem_total_bytes: 0,
            load_one: None,
            process_cpu_percent: 0.0,
            process_mem_bytes: 0,
            net_rx_bytes_per_sec: 0.0,
            net_tx_bytes_per_sec: 0.0,
        }
    })
});

macro_rules! replica_provider_registration_from_row {
    ($row:expr) => {{
        Ok(ReplicaProviderRegistration {
            replica_id: $row.get("replica_id"),
            provider_name: $row.get("provider_name"),
            provider: serde_json::from_str(&$row.get::<String, _>("provider_json")).unwrap_or(
                runinator_models::providers::ProviderMetadata {
                    name: $row.get("provider_name"),
                    actions: Vec::new(),
                    metadata: Default::default(),
                },
            ),
            first_registered_at: DateTime::<Utc>::from_timestamp(
                $row.get("first_registered_at"),
                0,
            )
            .unwrap_or_else(Utc::now),
            last_registered_at: DateTime::<Utc>::from_timestamp($row.get("last_registered_at"), 0)
                .unwrap_or_else(Utc::now),
            last_heartbeat_at: DateTime::<Utc>::from_timestamp($row.get("last_heartbeat_at"), 0)
                .unwrap_or_else(Utc::now),
        })
    }};
}

row_mapper!(row_to_replica_provider_registration(row) -> Result<ReplicaProviderRegistration, SendableError> {
    replica_provider_registration_from_row!(row)
});
