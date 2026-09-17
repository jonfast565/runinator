#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(super) struct ApprovalParams {
    pub(super) approval_type: Option<String>,
    pub(super) prompt: Option<String>,
    #[serde(flatten)]
    pub(super) metadata: Map,
}
