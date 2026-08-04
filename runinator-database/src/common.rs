use chrono::{DateTime, Utc};
use croner::Cron;
use runinator_comm::{WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::errors::SendableError;
use runinator_models::pipelines::PipelineTrigger;
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

/// extract a uuid-string field (e.g. a workflow-run/external-item key) as a `Uuid`.
pub(crate) fn json_opt_uuid(value: &Value, key: &str) -> Option<uuid::Uuid> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse().ok())
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

/// every cron occurrence strictly after `after` and at or before `until`, capped at `max`. the
/// second tuple element is true when the cap cut the range short, which tells the caller there is
/// still backlog to drain on the next pass.
pub(crate) fn cron_slots_between(
    cron_schedule: &str,
    after: DateTime<Utc>,
    until: DateTime<Utc>,
    max: i64,
) -> Result<(Vec<DateTime<Utc>>, bool), SendableError> {
    let cron = cron_schedule
        .parse::<Cron>()
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let mut slots = Vec::new();
    let mut cursor = after;
    loop {
        let next = cron
            .find_next_occurrence(&cursor, false)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        if next > until {
            return Ok((slots, false));
        }
        if slots.len() as i64 >= max {
            return Ok((slots, true));
        }
        slots.push(next);
        cursor = next;
    }
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

/// the run state a cron firing starts with, stamped with the schedule slot it stands for. a
/// catch-up or backfill run is created well after its slot, so the slot is the only way to tell
/// which occurrence a run belongs to.
pub(crate) fn trigger_state_for_slot(trigger: &WorkflowTrigger, slot: DateTime<Utc>) -> Value {
    let mut state = trigger_state(trigger);
    let Some(trigger_object) = state
        .get_mut("trigger")
        .and_then(|value| value.as_object_mut())
    else {
        return state;
    };
    trigger_object.insert("scheduled_for".into(), Value::from(slot.timestamp()));

    state
}

pub(crate) fn is_trigger_in_blackout(trigger: &WorkflowTrigger, now: DateTime<Utc>) -> bool {
    if let (Some(start), Some(end)) = (trigger.blackout_start, trigger.blackout_end) {
        return now >= start && now <= end;
    }
    false
}

pub(crate) fn pipeline_trigger_parameters(trigger: &PipelineTrigger) -> Value {
    trigger
        .configuration
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub(crate) fn pipeline_trigger_state(trigger: &PipelineTrigger) -> Value {
    runinator_models::json!({
        "trigger": {
            "id": trigger.id,
            "kind": trigger.kind,
            "metadata": trigger.metadata
        }
    })
}

pub(crate) fn is_pipeline_trigger_in_blackout(
    trigger: &PipelineTrigger,
    now: DateTime<Utc>,
) -> bool {
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
