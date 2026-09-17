#[allow(unused_imports)]
use super::*;

/// a persisted pipeline-level trigger. mirrors [`crate::workflows::WorkflowTrigger`] but is owned by a
/// pipeline: cron/manual start a pipeline run for `pipeline_id`; a `chained` trigger is target-keyed
/// (`pipeline_id` is the pipeline to start) with its source and `on` selector in `configuration`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineTrigger {
    pub id: Option<Uuid>,
    pub pipeline_id: Uuid,
    pub kind: WorkflowTriggerKind,
    pub enabled: bool,
    #[serde(default)]
    pub configuration: Value,
    pub next_execution: Option<DateTime<Utc>>,
    pub blackout_start: Option<DateTime<Utc>>,
    pub blackout_end: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl crate::validation::Validate for PipelineTrigger {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        crate::workflows::validate_trigger_window(self.blackout_start, self.blackout_end)?;
        crate::validation::dynamic_value("configuration", &self.configuration)?;
        crate::validation::dynamic_value("metadata", &self.metadata)?;
        Ok(())
    }
}
