use chrono::{DateTime, Utc};
use runinator_models::errors::SendableError;
use runinator_models::pipelines::PipelineTrigger;
use runinator_models::schedules::{ScheduleRecurrence, ScheduleSpec};
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

pub(crate) fn schedule_from_configuration(
    configuration: &Value,
) -> Result<ScheduleSpec, SendableError> {
    if let Some(schedule) = configuration.get("schedule") {
        return serde_json::from_value(schedule.clone().into())
            .map_err(|error| -> SendableError { Box::new(error) });
    }
    Ok(ScheduleSpec {
        recurrence: ScheduleRecurrence::Cron {
            expression: configuration
                .get("cron")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        timezone: "UTC".to_string(),
        duration_seconds: 0,
    })
}

pub(crate) fn exclusion_schedules(
    configuration: &Value,
) -> Result<Vec<ScheduleSpec>, SendableError> {
    let Some(value) = configuration.get("exclusions") else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value.clone().into())
        .map_err(|error| -> SendableError { Box::new(error) })
}

pub(crate) fn next_execution_for_schedule(
    schedule: &ScheduleSpec,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, SendableError> {
    runinator_scheduling::next_after(schedule, now)
        .map_err(|error| -> SendableError { Box::new(error) })
}

/// every cron occurrence strictly after `after` and at or before `until`, capped at `max`. the
/// second tuple element is true when the cap cut the range short, which tells the caller there is
/// still backlog to drain on the next pass.
pub(crate) fn schedule_slots_between(
    schedule: &ScheduleSpec,
    after: DateTime<Utc>,
    until: DateTime<Utc>,
    max: i64,
) -> Result<(Vec<DateTime<Utc>>, bool), SendableError> {
    runinator_scheduling::between(schedule, after, until, max)
        .map_err(|error| -> SendableError { Box::new(error) })
}

/// helpers over a workflow trigger's json configuration/blackout window. a local trait since
/// `WorkflowTrigger` lives in `runinator-models`, which stays free of database-layer behavior.
pub(crate) trait WorkflowTriggerExt {
    fn trigger_parameters(&self) -> Value;
    fn trigger_state(&self) -> Value;
    fn trigger_state_for_slot(&self, slot: DateTime<Utc>) -> Value;
    fn is_trigger_excluded(&self, slot: DateTime<Utc>) -> Result<bool, SendableError>;
    fn next_allowed_execution(
        &self,
        schedule: &ScheduleSpec,
        after: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, SendableError>;
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

    fn is_trigger_excluded(&self, slot: DateTime<Utc>) -> Result<bool, SendableError> {
        if let (Some(start), Some(end)) = (self.blackout_start, self.blackout_end)
            && slot >= start
            && slot < end
        {
            return Ok(true);
        }
        for exclusion in exclusion_schedules(&self.configuration)? {
            if runinator_scheduling::is_excluded(&exclusion, slot)
                .map_err(|error| -> SendableError { Box::new(error) })?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn next_allowed_execution(
        &self,
        schedule: &ScheduleSpec,
        after: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, SendableError> {
        let mut cursor = after;
        for _ in 0..10_000 {
            let next = next_execution_for_schedule(schedule, cursor)?;
            if !self.is_trigger_excluded(next)? {
                return Ok(next);
            }
            cursor = next;
        }
        Err(
            std::io::Error::other("schedule exclusions contain too many consecutive occurrences")
                .into(),
        )
    }
}

/// mirror of `WorkflowTriggerExt` for pipeline triggers; kept as a separate trait since the two
/// types share no supertype to hang one impl off of.
pub(crate) trait PipelineTriggerExt {
    fn pipeline_trigger_parameters(&self) -> Value;
    fn pipeline_trigger_state(&self) -> Value;
    fn is_pipeline_trigger_excluded(&self, slot: DateTime<Utc>) -> Result<bool, SendableError>;
    fn next_allowed_pipeline_execution(
        &self,
        schedule: &ScheduleSpec,
        after: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, SendableError>;
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

    fn is_pipeline_trigger_excluded(&self, slot: DateTime<Utc>) -> Result<bool, SendableError> {
        if let (Some(start), Some(end)) = (self.blackout_start, self.blackout_end)
            && slot >= start
            && slot < end
        {
            return Ok(true);
        }
        for exclusion in exclusion_schedules(&self.configuration)? {
            if runinator_scheduling::is_excluded(&exclusion, slot)
                .map_err(|error| -> SendableError { Box::new(error) })?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn next_allowed_pipeline_execution(
        &self,
        schedule: &ScheduleSpec,
        after: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, SendableError> {
        let mut cursor = after;
        for _ in 0..10_000 {
            let next = next_execution_for_schedule(schedule, cursor)?;
            if !self.is_pipeline_trigger_excluded(next)? {
                return Ok(next);
            }
            cursor = next;
        }
        Err(
            std::io::Error::other("schedule exclusions contain too many consecutive occurrences")
                .into(),
        )
    }
}

pub(crate) fn status_list(statuses: &[WorkflowStatus]) -> String {
    statuses
        .iter()
        .map(|status| format!("'{}'", status.as_str().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}
