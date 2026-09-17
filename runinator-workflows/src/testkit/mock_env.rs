#[allow(unused_imports)]
use super::*;

pub struct MockEnv {
    pub(super) config: Value,
    pub(super) outcomes: HashMap<String, NodeOutcome>,
    pub(super) default_outcome: NodeOutcome,
}

impl MockEnv {
    /// build a mock env from a `config.*` tree and per-node outcomes keyed by node id.
    pub fn new(config: Value, outcomes: HashMap<String, NodeOutcome>) -> Self {
        Self {
            config,
            outcomes,
            default_outcome: NodeOutcome::succeeded(Value::Null),
        }
    }

    /// override the outcome used for a node the spec does not explicitly mock.
    pub fn with_default(mut self, outcome: NodeOutcome) -> Self {
        self.default_outcome = outcome;
        self
    }

    pub(super) fn outcome_for(&self, node_id: &str) -> NodeOutcome {
        self.outcomes
            .get(node_id)
            .cloned()
            .unwrap_or_else(|| self.default_outcome.clone())
    }
}

impl SimulationEnv for MockEnv {
    fn config_tree(&mut self) -> Value {
        self.config.clone()
    }

    fn evaluate_action(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome {
        self.outcome_for(&request.node.id)
    }

    fn resolve_park(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome {
        self.outcome_for(&request.node.id)
    }
}
