#[allow(unused_imports)]
use super::*;

pub struct NodeEvalRequest<'a> {
    /// the node being resolved.
    pub node: &'a WorkflowNode,
    /// the node's action configuration / parameters already resolved against the run context.
    pub resolved: Value,
    /// the full run context at this point in the walk (`input`, `steps`, `config`, ...).
    pub context: &'a Value,
}
