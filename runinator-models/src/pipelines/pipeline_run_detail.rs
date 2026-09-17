#[allow(unused_imports)]
use super::*;

/// a pipeline run with the member workflow runs it started. mirrors the workflow-run detail shape so
/// the UI can render the same list+detail layout and click through from a member step to its run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRunDetail {
    pub run: PipelineRun,
    pub members: Vec<WorkflowRun>,
    #[serde(default)]
    pub attempts: Vec<PipelineMemberAttempt>,
    #[serde(default)]
    pub edges: Vec<PipelineRunEdgeState>,
    #[serde(default)]
    pub joins: Vec<PipelineRunJoinState>,
}
