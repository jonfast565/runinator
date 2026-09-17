#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct SignalParameters {
    pub name: String,
    /// unresolved correlation-key expression (often a ref); the reducer resolves it at park time.
    pub correlation_key: WorkflowExpression,
}
