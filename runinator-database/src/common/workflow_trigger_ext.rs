#[allow(unused_imports)]
use super::*;

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
