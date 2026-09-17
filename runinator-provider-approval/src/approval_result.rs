#[allow(unused_imports)]
use super::*;

#[derive(Serialize)]
pub(super) struct ApprovalResult {
    pub(super) approval_type: String,
    pub(super) prompt: String,
    pub(super) metadata: Value,
}
