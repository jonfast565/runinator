#[allow(unused_imports)]
use super::*;

/// the result of walking a workflow with a `SimulationEnv`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SimulationRun {
    /// the terminal status the run settled on.
    pub status: WorkflowStatus,
    /// the ordered nodes visited.
    pub steps: Vec<SimStep>,
    /// the run's final output (from the last output node, else null).
    pub output: Value,
    /// set when the walk could not continue: an unsupported node kind, a missing node, or a node
    /// that blocked with no outgoing edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SimulationRun {
    /// true when a node with `node_id` was visited during the walk.
    pub fn reached(&self, node_id: &str) -> bool {
        self.steps.iter().any(|step| step.node_id == node_id)
    }

    /// the target the last visit to `node_id` routed to, if any. Used to assert which branch a
    /// condition/switch/toggle/percentage node took.
    pub fn branch_target(&self, node_id: &str) -> Option<&str> {
        self.steps
            .iter()
            .rev()
            .find(|step| step.node_id == node_id)
            .and_then(|step| step.next.as_deref())
    }

    /// the recorded output of the last visit to `node_id`, if any.
    pub fn node_output(&self, node_id: &str) -> Option<&Value> {
        self.steps
            .iter()
            .rev()
            .find(|step| step.node_id == node_id)
            .and_then(|step| step.output.as_ref())
    }
}
