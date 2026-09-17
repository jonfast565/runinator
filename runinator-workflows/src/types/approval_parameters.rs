#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalParameters {
    pub approval_type: String,
    pub prompt: String,
    pub metadata: Value,
}
