#[allow(unused_imports)]
use super::*;

/// request body for a server-side dry-run (branch preview). The `workflow` is walked with the
/// reducer's evaluators against live config, publishing no actions; `inputs` seed the run and an
/// optional `replay_run` replays that run's recorded node outputs so the walk follows real branches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSimulateRequest {
    pub workflow: WorkflowDefinition,
    #[serde(default)]
    pub inputs: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_run: Option<Uuid>,
}

impl crate::validation::Validate for WorkflowSimulateRequest {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        crate::validation::Validate::validate(&self.workflow)?;
        crate::validation::dynamic_value("inputs", &self.inputs)?;
        Ok(())
    }
}
