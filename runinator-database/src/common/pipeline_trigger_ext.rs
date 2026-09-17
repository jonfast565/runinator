#[allow(unused_imports)]
use super::*;

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
