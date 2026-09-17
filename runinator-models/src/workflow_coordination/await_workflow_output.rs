#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitWorkflowOutput {
    pub workflow_id: Uuid,
    pub matched_run_ids: Vec<Uuid>,
    pub mode: String,
    pub statuses: Vec<String>,
}
