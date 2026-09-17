#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraEnsureTransitionParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub transition_id: String,
    pub target_status: String,
    pub operation_key: Option<String>,
}
