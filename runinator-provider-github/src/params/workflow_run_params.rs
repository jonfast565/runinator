#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct WorkflowRunParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub run_id: String,
}
