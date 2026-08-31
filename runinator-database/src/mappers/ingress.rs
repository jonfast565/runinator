use super::*;

fallible_row_mapper!(row_to_ingress_admission(row) -> IngressAdmission {
    let workflow_id = row.get::<Option<Uuid>, _>("workflow_id");
    let pipeline_id = row.get::<Option<Uuid>, _>("pipeline_id");
    let org_scope = row.get::<String, _>("org_scope");
    let target = match (workflow_id, pipeline_id) {
        (Some(id), None) => IngressTarget { kind: IngressTargetKind::Workflow, id },
        (None, Some(id)) => IngressTarget { kind: IngressTargetKind::Pipeline, id },
        _ => return Err(Box::new(std::io::Error::other("invalid ingress target identity"))),
    };
    let status = match row.get::<String, _>("status").as_str() {
        "active" => IngressAdmissionStatus::Active,
        "terminal" => IngressAdmissionStatus::Terminal,
        value => return Err(Box::new(std::io::Error::other(format!("invalid ingress admission status '{value}'")))),
    };
    Ok(IngressAdmission {
        id: Some(row.get("id")),
        org_id: if org_scope.is_empty() { None } else { Some(Uuid::parse_str(&org_scope)?) },
        scope: row.get("scope"),
        correlation_key: row.get("correlation_key"),
        generation: row.get("generation"),
        target,
        status,
        workflow_run_id: row.get("workflow_run_id"),
        pipeline_run_id: row.get("pipeline_run_id"),
        policy: parse_json(row.get("policy")),
        created_at: DateTime::<Utc>::from_timestamp(row.get("created_at"), 0).unwrap_or_else(Utc::now),
        updated_at: DateTime::<Utc>::from_timestamp(row.get("updated_at"), 0).unwrap_or_else(Utc::now),
    })
});

fallible_row_mapper!(row_to_ingress_event(row) -> IngressInboxEntry {
    let disposition = match row.get::<String, _>("disposition").as_str() {
        "started" => IngressEventDisposition::Started,
        "recorded" => IngressEventDisposition::Recorded,
        "queued" => IngressEventDisposition::Queued,
        "interrupt_requested" => IngressEventDisposition::InterruptRequested,
        "requeued" => IngressEventDisposition::Requeued,
        "rejected" => IngressEventDisposition::Rejected,
        value => return Err(Box::new(std::io::Error::other(format!("invalid ingress event disposition '{value}'")))),
    };
    let queue_state = match row.get::<String, _>("queue_state").as_str() {
        "none" => IngressQueueState::None,
        "queued" => IngressQueueState::Queued,
        "claimed" => IngressQueueState::Claimed,
        "promoted" => IngressQueueState::Promoted,
        value => return Err(Box::new(std::io::Error::other(format!("invalid ingress queue state '{value}'")))),
    };
    Ok(IngressInboxEntry {
        id: row.get("id"), admission_id: row.get("admission_id"), sequence: row.get("sequence"),
        generation: row.get("generation"), source: row.get("source"), event_id: row.get("event_id"),
        event_type: row.get("event_type"), correlation_key: row.get("correlation_key"),
        payload: parse_json(row.get("payload")),
        provenance: parse_json(row.get("provenance")),
        occurred_at: row.get::<Option<i64>, _>("occurred_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
        received_at: DateTime::<Utc>::from_timestamp(row.get("received_at"), 0).unwrap_or_else(Utc::now),
        disposition, queue_state, queue_position: None,
        promoted_generation: row.get("promoted_generation"), workflow_run_id: row.get("workflow_run_id"),
        pipeline_run_id: row.get("pipeline_run_id"),
    })
});
