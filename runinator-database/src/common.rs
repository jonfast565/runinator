use chrono::{DateTime, Utc};
use croner::Cron;
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

/// extract a UUID-string field (e.g. a workflow-run/external-item key) as a `Uuid`.
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

/// helpers over a workflow trigger's json configuration/blackout window. a local trait since
/// `WorkflowTrigger` lives in `runinator-models`, which stays free of database-layer behavior.
pub(crate) trait WorkflowTriggerExt {
    fn trigger_parameters(&self) -> Value;
    fn trigger_state(&self) -> Value;
    fn trigger_state_for_slot(&self, slot: DateTime<Utc>) -> Value;
    fn is_trigger_in_blackout(&self, now: DateTime<Utc>) -> bool;
}

impl WorkflowTriggerExt for WorkflowTrigger {
    fn trigger_parameters(&self) -> Value {
        self.configuration
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()))
    }

    fn trigger_state(&self) -> Value {
        runinator_models::json!({
            "control": { "pause_requested": false },
            "trigger": {
                "id": self.id,
                "kind": self.kind,
                "metadata": self.metadata
            }
        })
    }

    /// the run state a cron firing starts with, stamped with the schedule slot it stands for. a
    /// catch-up or backfill run is created well after its slot, so the slot is the only way to tell
    /// which occurrence a run belongs to.
    fn trigger_state_for_slot(&self, slot: DateTime<Utc>) -> Value {
        let mut state = self.trigger_state();
        let Some(trigger_object) = state
            .get_mut("trigger")
            .and_then(|value| value.as_object_mut())
        else {
            return state;
        };
        trigger_object.insert("scheduled_for".into(), Value::from(slot.timestamp()));

        state
    }

    fn is_trigger_in_blackout(&self, now: DateTime<Utc>) -> bool {
        if let (Some(start), Some(end)) = (self.blackout_start, self.blackout_end) {
            return now >= start && now <= end;
        }
        false
    }
}

/// mirror of `WorkflowTriggerExt` for pipeline triggers; kept as a separate trait since the two
/// types share no supertype to hang one impl off of.
pub(crate) trait PipelineTriggerExt {
    fn pipeline_trigger_parameters(&self) -> Value;
    fn pipeline_trigger_state(&self) -> Value;
    fn is_pipeline_trigger_in_blackout(&self, now: DateTime<Utc>) -> bool;
}

impl PipelineTriggerExt for PipelineTrigger {
    fn pipeline_trigger_parameters(&self) -> Value {
        self.configuration
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()))
    }

    fn pipeline_trigger_state(&self) -> Value {
        runinator_models::json!({
            "trigger": {
                "id": self.id,
                "kind": self.kind,
                "metadata": self.metadata
            }
        })
    }

    fn is_pipeline_trigger_in_blackout(&self, now: DateTime<Utc>) -> bool {
        if let (Some(start), Some(end)) = (self.blackout_start, self.blackout_end) {
            return now >= start && now <= end;
        }
        false
    }
}

pub(crate) fn status_list(statuses: &[WorkflowStatus]) -> String {
    statuses
        .iter()
        .map(|status| format!("'{}'", status.as_str().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}
