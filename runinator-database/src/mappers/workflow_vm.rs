use super::*;
use runinator_comm::EffectDispatchRecord;
use runinator_models::workflow_vm::{
    WORKFLOW_EFFECT_PROTOCOL_VERSION, WORKFLOW_JOURNAL_VERSION, WorkflowContinuation,
    WorkflowContinuationStatus, WorkflowEffect, WorkflowEffectRequest, WorkflowEffectStatus,
    WorkflowJournalEntry, WorkflowJournalRecord, WorkflowModule,
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

fn u32_column(value: i64, column: &str) -> Result<u32, SendableError> {
    u32::try_from(value).map_err(|_| corrupt(format!("{column} is outside the u32 range")))
}

fn u64_column(value: i64, column: &str) -> Result<u64, SendableError> {
    u64::try_from(value).map_err(|_| corrupt(format!("{column} must not be negative")))
}

fallible_row_mapper!(row_to_workflow_module(row) -> WorkflowModule {
    let module = decode::<WorkflowModule>(&row.get::<String, _>("module_json"))?;
    let version = u32_column(row.get("version"), "module version")?;
    if module.version != version {
        return Err(corrupt("workflow module row version does not match its payload"));
    }
    module.ensure_supported().map_err(corrupt)?;
    Ok(module)
});

fallible_row_mapper!(row_to_workflow_continuation(row) -> WorkflowContinuation {
    let module_version = u32_column(row.get("module_version"), "continuation module version")?;
    let row_status: WorkflowContinuationStatus = decode(&format!("\"{}\"", row.get::<String, _>("status")))?;
    let row_revision = u64_column(row.get("version"), "continuation revision")?;
    let continuation = continuation(row.get("id"), row.get("workflow_run_id"), module_version, row.get("continuation_json"))?;
    if continuation.status != row_status || continuation.revision != row_revision {
        return Err(corrupt("continuation row status or revision does not match its payload"));
    }
    Ok(continuation)
});

fallible_row_mapper!(row_to_workflow_effect(row) -> WorkflowEffect {
    let version = u32_column(row.get("version"), "effect version")?;
    if version != WORKFLOW_EFFECT_PROTOCOL_VERSION {
        return Err(corrupt(format!("unsupported stored effect version {version}")));
    }
    Ok(WorkflowEffect {
        version,
        id: row.get("id"), workflow_run_id: row.get("workflow_run_id"),
        continuation_id: row.get("continuation_id"), sequence: u64_column(row.get("sequence"), "effect sequence")?,
        attempt: u32_column(row.get("attempt"), "effect attempt")?,
        request: decode::<WorkflowEffectRequest>(&row.get::<String, _>("request_json"))?,
        status: decode::<WorkflowEffectStatus>(&format!("\"{}\"", row.get::<String, _>("status")))?,
        result: row.get::<Option<String>, _>("result_json").map(|raw| decode(&raw)).transpose()?,
        message: row.get("message"), created_at: row.get("created_at"), updated_at: row.get("updated_at"), finished_at: row.get("finished_at"),
    })
});

fallible_row_mapper!(row_to_workflow_journal_record(row) -> WorkflowJournalRecord {
    let version = u32_column(row.get("version"), "journal version")?;
    if version != WORKFLOW_JOURNAL_VERSION {
        return Err(corrupt(format!("unsupported stored journal version {version}")));
    }
    Ok(WorkflowJournalRecord {
        version,
        id: row.get("id"), workflow_run_id: row.get("workflow_run_id"), sequence: u64_column(row.get("sequence"), "journal sequence")?,
        continuation_id: row.get("continuation_id"), effect_id: row.get("effect_id"),
        entry: decode::<WorkflowJournalEntry>(&row.get::<String, _>("entry_json"))?,
        created_at: row.get("created_at"),
    })
});

fallible_row_mapper!(row_to_workflow_effect_dispatch(row) -> EffectDispatchRecord {
    let command: runinator_comm::EffectCommand = decode(&row.get::<String, _>("command_json"))?;
    command.ensure_supported().map_err(corrupt)?;
    Ok(EffectDispatchRecord {
        id: row.get("id"),
        effect_id: row.get("effect_id"),
        dedupe_key: row.get("dedupe_key"),
        command,
        attempts: row.get("attempts"),
        created_at: DateTime::from_timestamp(row.get("created_at"), 0)
            .ok_or_else(|| corrupt("invalid effect dispatch creation timestamp"))?,
        updated_at: DateTime::from_timestamp(row.get("updated_at"), 0)
            .ok_or_else(|| corrupt("invalid effect dispatch update timestamp"))?,
        published_at: row.get::<Option<i64>, _>("published_at")
            .map(|value| DateTime::from_timestamp(value, 0)
                .ok_or_else(|| corrupt("invalid effect dispatch publication timestamp")))
            .transpose()?,
        last_error: row.get("last_error"),
        claimed_by: row.get("claimed_by"),
        claimed_until: row.get::<Option<i64>, _>("claimed_until")
            .map(|value| DateTime::from_timestamp(value, 0)
                .ok_or_else(|| corrupt("invalid effect dispatch lease timestamp")))
            .transpose()?,
    })
});
