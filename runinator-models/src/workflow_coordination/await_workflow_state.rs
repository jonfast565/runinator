#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitWorkflowState {
    pub workflow_id: Uuid,
    pub workflow_name: String,
    /// When awaiting a RexRap task handle, join exactly this child run rather than every run of
    /// the workflow. Optional for backward-compatible persisted await state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_run_id: Option<Uuid>,
    /// When awaiting a provider task handle, join exactly this independent task run. This stays
    /// alongside `exact_run_id` so previously persisted workflow-await state remains readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_task_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_unix: Option<i64>,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}
