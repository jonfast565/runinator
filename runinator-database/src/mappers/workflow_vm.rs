use super::*;
use runinator_models::workflow_vm::{
    WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowContinuation, WorkflowEffect, WorkflowEffectRequest,
    WorkflowEffectStatus, WorkflowJournalEntry, WorkflowJournalRecord,
};

fn corrupt(error: impl std::fmt::Display) -> SendableError {
    crate::errors::WORKFLOW_VM_CORRUPT_STATE.error(error)
}

fn continuation(
    id: Uuid,
    workflow_run_id: Uuid,
    module_version: u32,
    raw: String,
) -> Result<WorkflowContinuation, SendableError> {
    let value: WorkflowContinuation = serde_json::from_str(&raw).map_err(corrupt)?;
    if value.id != id
        || value.workflow_run_id != workflow_run_id
        || value.module_version != module_version
        || !value.is_supported()
    {
        return Err(corrupt(
            "continuation row identity or version does not match its payload",
        ));
    }
    Ok(value)
}

fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, SendableError> {
    serde_json::from_str(raw).map_err(corrupt)
}

fallible_row_mapper!(row_to_workflow_continuation(row) -> WorkflowContinuation {
    continuation(row.get("id"), row.get("workflow_run_id"), row.get::<i64, _>("module_version") as u32, row.get("continuation_json"))
});

fallible_row_mapper!(row_to_workflow_effect(row) -> WorkflowEffect {
    let version = row.get::<i64, _>("version") as u32;
    if version != WORKFLOW_EFFECT_PROTOCOL_VERSION {
        return Err(corrupt(format!("unsupported stored effect version {version}")));
    }
    Ok(WorkflowEffect {
        version,
        id: row.get("id"), workflow_run_id: row.get("workflow_run_id"),
        continuation_id: row.get("continuation_id"), sequence: row.get::<i64, _>("sequence") as u64,
        attempt: row.get::<i64, _>("attempt") as u32,
        request: decode::<WorkflowEffectRequest>(&row.get::<String, _>("request_json"))?,
        status: decode::<WorkflowEffectStatus>(&format!("\"{}\"", row.get::<String, _>("status")))?,
        result: row.get::<Option<String>, _>("result_json").map(|raw| decode(&raw)).transpose()?,
        message: row.get("message"), created_at: row.get("created_at"), updated_at: row.get("updated_at"), finished_at: row.get("finished_at"),
    })
});

fallible_row_mapper!(row_to_workflow_journal_record(row) -> WorkflowJournalRecord {
    let version = row.get::<i64, _>("version") as u32;
    if version != WORKFLOW_EFFECT_PROTOCOL_VERSION {
        return Err(corrupt(format!("unsupported stored journal version {version}")));
    }
    Ok(WorkflowJournalRecord {
        version,
        id: row.get("id"), workflow_run_id: row.get("workflow_run_id"), sequence: row.get::<i64, _>("sequence") as u64,
        continuation_id: row.get("continuation_id"), effect_id: row.get("effect_id"),
        entry: decode::<WorkflowJournalEntry>(&row.get::<String, _>("entry_json"))?,
        created_at: row.get("created_at"),
    })
});
