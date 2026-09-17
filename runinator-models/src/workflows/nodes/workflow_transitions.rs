#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowTransitions {
    #[serde(default)]
    pub next: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub on_success: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub on_failure: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub on_timeout: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub on_reject: Option<WorkflowNodeRef>,
    #[serde(default)]
    pub branches: Vec<WorkflowBranch>,
}
