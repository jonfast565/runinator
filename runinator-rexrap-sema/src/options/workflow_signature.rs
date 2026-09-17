#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowSignature {
    pub name: String,
    pub input: RuninatorType,
    pub output: RuninatorType,
}
