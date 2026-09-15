//! durable process diagnostics persistence and filtering.

use runinator_models::{
    diagnostics::{RuntimeLogPage, RuntimeLogQuery, RuntimeLogRecord},
    errors::SendableError,
    ingress_control::{BrokerMessageDirection, BrokerMessageRecord},
    value::Value,
};
use runinator_store::roles::{DeliveryStore, WorkflowVmStore};
use uuid::Uuid;

const DIAGNOSTICS_CHANNEL: &str = "diagnostics";
const MAX_FETCH: i64 = 1000;

/// Resolve an effect to the durable workflow run that owns its diagnostics.
pub(crate) async fn diagnostic_run_for_effect<T: WorkflowVmStore>(
    db: &T,
    effect_id: Uuid,
) -> Result<Option<Uuid>, SendableError> {
    Ok(db
        .fetch_workflow_effect(effect_id)
        .await?
        .map(|effect| effect.workflow_run_id))
}

/// Store a source-bound diagnostics batch idempotently.
pub(crate) async fn record_runtime_logs<T: DeliveryStore>(
    db: &T,
    records: Vec<RuntimeLogRecord>,
) -> Result<(), SendableError> {
    for record in records {
        let payload = Value::from(serde_json::to_value(&record)?);
        db.record_broker_message(BrokerMessageRecord {
            adapter_id: None,
            poll_attempt_id: None,
            id: record.event_id,
            channel: DIAGNOSTICS_CHANNEL.into(),
            direction: BrokerMessageDirection::Received,
            message_kind: "runtime_log".into(),
            workflow_run_id: record.workflow_run_id,
            delivery_id: record.replica_id,
            dedupe_key: record.runtime_id.clone(),
            trace_id: record.trace_id,
            payload,
            occurred_at: record.occurred_at,
        })
        .await?;
    }
    Ok(())
}

/// Read and filter retained runtime diagnostics, newest first.
pub(crate) async fn fetch_runtime_logs<T: DeliveryStore>(
    db: &T,
    query: &RuntimeLogQuery,
    correlated_run: Option<Uuid>,
    limit: i64,
    retention_seconds: i64,
) -> Result<RuntimeLogPage, SendableError> {
    let records = db
        .fetch_broker_messages(
            correlated_run,
            None,
            None,
            Some(DIAGNOSTICS_CHANNEL.into()),
            MAX_FETCH,
        )
        .await?;
    let source = query.source.as_deref().map(str::to_lowercase);
    let level = query.level.as_deref().map(str::to_lowercase);
    let text = query.text.as_deref().map(str::to_lowercase);
    let mut after_cursor = query.cursor.is_none();
    let mut records = records
        .into_iter()
        .filter_map(|stored| serde_json::from_value::<RuntimeLogRecord>(stored.payload.into()).ok())
        .filter(|record| {
            if after_cursor {
                return true;
            }
            if query
                .cursor
                .as_deref()
                .is_some_and(|cursor| cursor == record.event_id.to_string())
            {
                after_cursor = true;
            }
            false
        })
        .filter(|record| {
            query
                .effect_id
                .is_none_or(|id| record.effect_id == Some(id))
        })
        .filter(|record| query.from.is_none_or(|from| record.occurred_at >= from))
        .filter(|record| query.until.is_none_or(|until| record.occurred_at <= until))
        .filter(|record| {
            source
                .as_ref()
                .is_none_or(|value| record.source.to_lowercase().contains(value))
        })
        .filter(|record| {
            level
                .as_ref()
                .is_none_or(|value| record.level.eq_ignore_ascii_case(value))
        })
        .filter(|record| {
            text.as_ref()
                .is_none_or(|value| record.message.to_lowercase().contains(value))
        })
        .take(limit as usize + 1)
        .collect::<Vec<_>>();
    let next_cursor =
        (records.len() > limit as usize).then(|| records[limit as usize - 1].event_id.to_string());
    records.truncate(limit as usize);
    let dropped = records.iter().map(|record| record.dropped_before).sum();
    Ok(RuntimeLogPage {
        records,
        next_cursor,
        dropped,
        retention_seconds,
    })
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
