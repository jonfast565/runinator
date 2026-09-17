#[allow(unused_imports)]
use super::*;

pub trait SimulationEnv {
    /// the `config.*` reference tree merged into every node's context. Defaults to empty.
    fn config_tree(&mut self) -> Value {
        Value::Object(Map::new())
    }

    /// resolve a task (action) node: its simulated status and output.
    fn evaluate_action(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome;

    /// resolve a parked node (approval/gate/signal/input/mutex/...). Defaults to succeeding with a
    /// null output so a park never blocks a simulation unless an env overrides it.
    fn resolve_park(&mut self, _request: &NodeEvalRequest<'_>) -> NodeOutcome {
        NodeOutcome::succeeded(Value::Null)
    }
}
