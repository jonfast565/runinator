use chrono::{DateTime, Utc};
use croner::Cron;
use runinator_comm::{WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::errors::SendableError;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowStatus, WorkflowTrigger};

pub(crate) fn json_str(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn json_opt_str(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn json_opt_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

pub(crate) fn json_metadata(value: &Value) -> String {
    value
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()))
        .to_string()
}

pub(crate) fn workflow_result_event_type(event: &WorkflowResultEvent) -> &'static str {
    match &event.kind {
        WorkflowResultEventKind::Status { .. } => "status",
        WorkflowResultEventKind::Chunk { .. } => "chunk",
        WorkflowResultEventKind::Artifact { .. } => "artifact",
    }
}

pub(crate) fn next_execution_for_cron(
    cron_schedule: &str,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, SendableError> {
    let cron = cron_schedule
        .parse::<Cron>()
        .map_err(|err| -> SendableError { Box::new(err) })?;
    cron.find_next_occurrence(&now, false)
        .map_err(|err| -> SendableError { Box::new(err) })
}

pub(crate) fn trigger_parameters(trigger: &WorkflowTrigger) -> Value {
    trigger
        .configuration
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub(crate) fn trigger_state(trigger: &WorkflowTrigger) -> Value {
    runinator_models::json!({
        "control": { "pause_requested": false },
        "trigger": {
            "id": trigger.id,
            "kind": trigger.kind,
            "metadata": trigger.metadata
        }
    })
}

pub(crate) fn is_trigger_in_blackout(trigger: &WorkflowTrigger, now: DateTime<Utc>) -> bool {
    if let (Some(start), Some(end)) = (trigger.blackout_start, trigger.blackout_end) {
        return now >= start && now <= end;
    }
    false
}

pub(crate) fn status_list(statuses: &[WorkflowStatus]) -> String {
    statuses
        .iter()
        .map(|status| format!("'{}'", status.as_str().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}
