use std::collections::BTreeMap;

use super::*;
use runinator_models::orchestration::{
    AdapterDefinition, AdapterRevision, DeliverySemantics, ExternalOperation,
    ExternalOperationStatus, OrchestrationBinding, OrchestrationCommand,
    OrchestrationCommandStatus, OrchestrationEpoch, OrchestrationEventReduction,
    OrchestrationEvidence, OrchestrationPendingIntent, OrchestrationPolicy, OrchestrationStatus,
};

fn timestamp(value: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(value, 0).unwrap_or_else(Utc::now)
}

fn status(raw: &str) -> Result<OrchestrationStatus, SendableError> {
    let value = match raw {
        "pending" => OrchestrationStatus::Pending,
        "running" => OrchestrationStatus::Running,
        "waiting" => OrchestrationStatus::Waiting,
        "suspended" => OrchestrationStatus::Suspended,
        "completed" => OrchestrationStatus::Completed,
        "failed" => OrchestrationStatus::Failed,
        "terminated" => OrchestrationStatus::Terminated,
        other => {
            return Err(Box::new(std::io::Error::other(format!(
                "unknown orchestration status {other}"
            ))));
        }
    };
    Ok(value)
}

fallible_row_mapper!(row_to_orchestration_binding(row) -> OrchestrationBinding {
    let raw_policy = row.get::<String, _>("policy");
    let policy: OrchestrationPolicy = serde_json::from_str(&raw_policy)?;
    let budgets: BTreeMap<String, u32> = serde_json::from_str(&row.get::<String, _>("budgets"))?;
    Ok(OrchestrationBinding {
        id: row.get("id"),
        admission_id: row.get("admission_id"),
        org_id: row.get("org_id"),
        scope: row.get("scope"),
        correlation_key: row.get("correlation_key"),
        generation: row.get("generation"),
        pipeline_id: row.get("pipeline_id"),
        pipeline_revision: row.get("pipeline_revision"),
        pipeline_digest: row.get("pipeline_digest"),
        adapter_id: row.get("adapter_id"),
        adapter_revision: row.get("adapter_revision"),
        policy,
        status: status(&row.get::<String, _>("status"))?,
        current_phase: row.get("current_phase"),
        current_attempt: row.get("current_attempt"),
        current_epoch: row.get("current_epoch"),
        restart_member: row.get("restart_member"),
        resume_existing_epoch: row.get("resume_existing_epoch"),
        subject_revision: row.get("subject_revision"),
        resources: parse_json(row.get("resources")),
        budgets,
        last_reduced_sequence: row.get("last_reduced_sequence"),
        version: row.get("version"),
        reducer_lease_owner: row.get("reducer_lease_owner"),
        reducer_leased_until: row.get::<Option<i64>, _>("reducer_leased_until").map(timestamp),
        created_at: timestamp(row.get("created_at")),
        updated_at: timestamp(row.get("updated_at")),
        finished_at: row.get::<Option<i64>, _>("finished_at").map(timestamp),
    })
});

fallible_row_mapper!(row_to_orchestration_epoch(row) -> OrchestrationEpoch {
    Ok(OrchestrationEpoch {
        id: row.get("id"), binding_id: row.get("binding_id"), epoch: row.get("epoch"),
        pipeline_run_id: row.get("pipeline_run_id"), start_member: row.get("start_member"),
        parameters: parse_json(row.get("parameters")), status: row.get("status"), reason: row.get("reason"),
        created_at: timestamp(row.get("created_at")),
        started_at: row.get::<Option<i64>, _>("started_at").map(timestamp),
        finished_at: row.get::<Option<i64>, _>("finished_at").map(timestamp),
    })
});

fallible_row_mapper!(row_to_orchestration_reduction(row) -> OrchestrationEventReduction {
    Ok(OrchestrationEventReduction {
        id: row.get("id"), binding_id: row.get("binding_id"), inbox_event_id: row.get("inbox_event_id"),
        sequence: row.get("sequence"),
        matched_intents: serde_json::from_str(&row.get::<String, _>("matched_intents"))?,
        winner: row.get("winner"),
        suppressed_intents: serde_json::from_str(&row.get::<String, _>("suppressed_intents"))?,
        binding_version: row.get("binding_version"), disposition: row.get("disposition"),
        detail: parse_json(row.get("detail")), created_at: timestamp(row.get("created_at")),
    })
});

fallible_row_mapper!(row_to_orchestration_pending_intent(row) -> OrchestrationPendingIntent {
    Ok(OrchestrationPendingIntent {
        id: row.get("id"), binding_id: row.get("binding_id"), intent: row.get("intent"),
        priority: row.get::<i64, _>("priority") as i32,
        source_event_ids: serde_json::from_str(&row.get::<String, _>("source_event_ids"))?,
        latest_payload: parse_json(row.get("latest_payload")), wake_at: timestamp(row.get("wake_at")),
        created_at: timestamp(row.get("created_at")), updated_at: timestamp(row.get("updated_at")),
    })
});

fallible_row_mapper!(row_to_orchestration_command(row) -> OrchestrationCommand {
    let status = match row.get::<String, _>("status").as_str() {
        "pending" => OrchestrationCommandStatus::Pending,
        "claimed" => OrchestrationCommandStatus::Claimed,
        "succeeded" => OrchestrationCommandStatus::Succeeded,
        "failed" => OrchestrationCommandStatus::Failed,
        "superseded" => OrchestrationCommandStatus::Superseded,
        other => return Err(Box::new(std::io::Error::other(format!("unknown orchestration command status {other}")))),
    };
    Ok(OrchestrationCommand {
        id: row.get("id"), binding_id: row.get("binding_id"), epoch: row.get("epoch"),
        command_type: row.get("command_type"), operation_key: row.get("operation_key"),
        payload: parse_json(row.get("payload")), status, attempts: row.get("attempts"),
        claimed_by: row.get("claimed_by"), claimed_until: row.get::<Option<i64>, _>("claimed_until").map(timestamp),
        result: parse_json(row.get("result")), created_at: timestamp(row.get("created_at")),
        updated_at: timestamp(row.get("updated_at")),
    })
});

fallible_row_mapper!(row_to_orchestration_evidence(row) -> OrchestrationEvidence {
    Ok(OrchestrationEvidence {
        id: row.get("id"), binding_id: row.get("binding_id"), epoch: row.get("epoch"),
        kind: row.get("kind"), subject_revision: row.get("subject_revision"), payload: parse_json(row.get("payload")),
        source_event_id: row.get("source_event_id"), created_at: timestamp(row.get("created_at")),
    })
});

fallible_row_mapper!(row_to_orchestration_adapter(row) -> AdapterDefinition {
    Ok(AdapterDefinition {
        id: row.get("id"), org_id: row.get("org_id"), name: row.get("name"), kind: row.get("kind"),
        current_revision: row.get("current_revision"), enabled: row.get("enabled"),
        endpoint_identity: row.get("endpoint_identity"), has_admitted_binding: row.get("has_admitted_binding"),
        created_at: timestamp(row.get("created_at")), updated_at: timestamp(row.get("updated_at")),
    })
});

fallible_row_mapper!(row_to_orchestration_adapter_revision(row) -> AdapterRevision {
    Ok(AdapterRevision {
        id: row.get("id"), adapter_id: row.get("adapter_id"), revision: row.get("revision"),
        kind_version: row.get("kind_version"), configuration: parse_json(row.get("configuration")),
        secret_bindings: serde_json::from_str(&row.get::<String, _>("secret_bindings"))?,
        identity_configuration: parse_json(row.get("identity_configuration")),
        created_at: timestamp(row.get("created_at")), actor_id: row.get("actor_id"),
    })
});

fallible_row_mapper!(row_to_external_operation(row) -> ExternalOperation {
    let semantics = match row.get::<String, _>("semantics").as_str() {
        "at_least_once" => DeliverySemantics::AtLeastOnce,
        "idempotent" => DeliverySemantics::Idempotent,
        "reconcilable" => DeliverySemantics::Reconcilable,
        other => return Err(Box::new(std::io::Error::other(format!("unknown delivery semantics {other}")))),
    };
    let status = match row.get::<String, _>("status").as_str() {
        "pending" => ExternalOperationStatus::Pending,
        "running" => ExternalOperationStatus::Running,
        "waiting" => ExternalOperationStatus::Waiting,
        "succeeded" => ExternalOperationStatus::Succeeded,
        "failed" => ExternalOperationStatus::Failed,
        other => return Err(Box::new(std::io::Error::other(format!("unknown external operation status {other}")))),
    };
    Ok(ExternalOperation {
        id: row.get("id"), binding_id: row.get("binding_id"), epoch: row.get("epoch"),
        workflow_run_id: row.get("workflow_run_id"), effect_id: row.get("effect_id"),
        operation_key: row.get("operation_key"),
        provider: row.get("provider"), action: row.get("action"), semantics, attempt: row.get("attempt"),
        status, ambiguous: row.get("ambiguous"), provenance: parse_json(row.get("provenance")),
        receipt: parse_json(row.get("receipt")), created_at: timestamp(row.get("created_at")),
        updated_at: timestamp(row.get("updated_at")),
    })
});
