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

fn ingress_target(kind: String, id: Uuid) -> Result<IngressTarget, SendableError> {
    let kind = match kind.as_str() {
        "workflow" => IngressTargetKind::Workflow,
        "pipeline" => IngressTargetKind::Pipeline,
        value => {
            return Err(Box::new(std::io::Error::other(format!(
                "invalid ingress control target kind '{value}'"
            ))));
        }
    };
    Ok(IngressTarget { kind, id })
}

fn owner_scope(kind: String, id: Option<Uuid>) -> Result<ScopeRef, SendableError> {
    let kind = ScopeKind::from_str_lossy(&kind).ok_or_else(|| {
        Box::new(std::io::Error::other("invalid ingress control scope kind")) as SendableError
    })?;
    ScopeRef::new(kind, id).ok_or_else(|| {
        Box::new(std::io::Error::other(
            "invalid ingress control scope identity",
        )) as SendableError
    })
}

fn gate_mode(value: String) -> Result<ExternalIngressGateMode, SendableError> {
    match value.as_str() {
        "disabled" => Ok(ExternalIngressGateMode::Disabled),
        "paused" => Ok(ExternalIngressGateMode::Paused),
        "review" => Ok(ExternalIngressGateMode::Review),
        value => Err(Box::new(std::io::Error::other(format!(
            "invalid ingress gate mode '{value}'"
        )))),
    }
}

fn control_state(value: String) -> Result<IngressControlState, SendableError> {
    match value.as_str() {
        "held" => Ok(IngressControlState::Held),
        "approved" => Ok(IngressControlState::Approved),
        "applying" => Ok(IngressControlState::Applying),
        "applied" => Ok(IngressControlState::Applied),
        "dropped" => Ok(IngressControlState::Dropped),
        "failed" => Ok(IngressControlState::Failed),
        value => Err(Box::new(std::io::Error::other(format!(
            "invalid ingress control state '{value}'"
        )))),
    }
}

fallible_row_mapper!(row_to_external_ingress_gate(row) -> ExternalIngressGate {
    Ok(ExternalIngressGate {
        target: ingress_target(row.get("target_kind"), row.get("target_id"))?,
        owner_scope: owner_scope(row.get("owner_scope_kind"), row.get("owner_scope_id"))?,
        mode: gate_mode(row.get("mode"))?,
        updated_by: row.get("updated_by"),
        updated_at: DateTime::<Utc>::from_timestamp(row.get("updated_at"), 0).unwrap_or_else(Utc::now),
    })
});

fallible_row_mapper!(row_to_external_ingress_record(row) -> ExternalIngressRecord {
    Ok(ExternalIngressRecord {
        id: row.get("id"),
        target: ingress_target(row.get("target_kind"), row.get("target_id"))?,
        owner_scope: owner_scope(row.get("owner_scope_kind"), row.get("owner_scope_id"))?,
        gate_mode: gate_mode(row.get("gate_mode"))?,
        event: runinator_models::orchestration::IngressEvent {
            source: row.get("source"), event_id: row.get("event_id"), event_type: row.get("event_type"),
            correlation_key: row.get("correlation_key"), payload: parse_json(row.get("payload")),
            provenance: parse_json(row.get("provenance")),
            occurred_at: row.get::<Option<i64>, _>("occurred_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
        },
        state: control_state(row.get("state"))?, queue_position: None,
        reviewed_by: row.get("reviewed_by"), last_error: row.get("last_error"),
        received_at: DateTime::<Utc>::from_timestamp(row.get("received_at"), 0).unwrap_or_else(Utc::now),
        resolved_at: row.get::<Option<i64>, _>("resolved_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
    })
});

fn broker_session_mode(value: String) -> Result<BrokerIngressSessionMode, SendableError> {
    match value.as_str() {
        "off" => Ok(BrokerIngressSessionMode::Off),
        "observe" => Ok(BrokerIngressSessionMode::Observe),
        "hold_orchestration_nudges" => Ok(BrokerIngressSessionMode::HoldOrchestrationNudges),
        value => Err(Box::new(std::io::Error::other(format!(
            "invalid broker ingress session mode '{value}'"
        )))),
    }
}

fallible_row_mapper!(row_to_broker_ingress_session(row) -> BrokerIngressSession {
    Ok(BrokerIngressSession {
        scope: owner_scope(row.get("scope_kind"), row.get("scope_id"))?,
        mode: broker_session_mode(row.get("mode"))?, updated_by: row.get("updated_by"),
        updated_at: DateTime::<Utc>::from_timestamp(row.get("updated_at"), 0).unwrap_or_else(Utc::now),
    })
});

fallible_row_mapper!(row_to_broker_ingress_record(row) -> BrokerIngressRecord {
    Ok(BrokerIngressRecord {
        id: row.get("id"), scope: owner_scope(row.get("scope_kind"), row.get("scope_id"))?,
        delivery_id: row.get("delivery_id"), dedupe_key: row.get("dedupe_key"),
        command_kind: row.get("command_kind"), command: parse_json(row.get("command")),
        state: control_state(row.get("state"))?, reviewed_by: row.get("reviewed_by"),
        last_error: row.get("last_error"),
        received_at: DateTime::<Utc>::from_timestamp(row.get("received_at"), 0).unwrap_or_else(Utc::now),
        resolved_at: row.get::<Option<i64>, _>("resolved_at").and_then(|value| DateTime::<Utc>::from_timestamp(value, 0)),
    })
});
