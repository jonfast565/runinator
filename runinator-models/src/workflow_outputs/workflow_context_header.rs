#[allow(unused_imports)]
use super::*;

/// the `workflow` entry injected into the template-evaluation scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContextHeader {
    pub run_id: Uuid,
    pub workflow_id: Uuid,
    pub state: Value,
}

impl WorkflowContextHeader {
    pub fn runinator_type() -> RuninatorType {
        RuninatorType::structure([
            ("run_id", RuninatorType::String),
            ("workflow_id", RuninatorType::String),
            ("state", RuninatorType::Any),
        ])
    }
}
