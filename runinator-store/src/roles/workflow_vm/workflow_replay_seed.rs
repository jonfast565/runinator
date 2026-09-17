#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct WorkflowReplaySeed {
    pub locals: std::collections::BTreeMap<String, Value>,
    pub stack: Vec<Value>,
}
