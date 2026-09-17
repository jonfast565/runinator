#[allow(unused_imports)]
use super::*;

/// A result labelled with a fork or map index. A vector is intentional: JSON object keys are
/// strings, while this representation preserves a numeric index and a deterministic order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowIndexedValue {
    pub index: u64,
    pub value: Value,
}
