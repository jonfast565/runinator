#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowNode {
    pub id: String,
    pub kind: WorkflowNodeKind,
    #[serde(default)]
    pub skipped: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub action: Option<WorkflowAction>,
    #[serde(default)]
    pub parameters: WorkflowObject,
    #[serde(default)]
    pub wait: WorkflowWait,
    #[serde(default)]
    pub condition: WorkflowCondition,
    #[serde(default)]
    pub transitions: WorkflowTransitions,
    #[serde(default)]
    pub retry: WorkflowRetry,
    #[serde(default)]
    pub timeout_seconds: Option<i64>,
    #[serde(default)]
    pub max_iterations: Option<i64>,
    #[serde(default)]
    pub subflow_id: Option<Uuid>,
    #[serde(default)]
    pub subflow: WorkflowSubflow,
    #[serde(default)]
    pub reentry: WorkflowReentry,
    /// compensating action recorded when this node succeeds; run in reverse on saga rollback when a
    /// later step drives the run to a failed terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<WorkflowAction>,
}

impl WorkflowNode {
    /// the visit bound used by iterative control flow, regardless of its legacy wire location.
    pub fn iteration_limit(&self) -> Option<i64> {
        self.max_iterations.or_else(|| {
            (self.reentry.enabled && self.reentry.max_visits > 0).then_some(self.reentry.max_visits)
        })
    }
}
