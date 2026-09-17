#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunDetail {
    pub run: WorkflowRun,
    pub nodes: Vec<WorkflowNodeRun>,
    #[serde(default)]
    pub continuations: Vec<WorkflowContinuation>,
    #[serde(default)]
    pub effects: Vec<WorkflowEffect>,
    #[serde(default)]
    pub journal: Vec<WorkflowJournalRecord>,
    #[serde(default)]
    pub vm_cursors: Vec<WorkflowVmCursor>,
    #[serde(default)]
    pub ai_usage: Option<Value>,
}
