#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct DispatchParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub workflow_id: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub inputs: Option<Value>,
}
