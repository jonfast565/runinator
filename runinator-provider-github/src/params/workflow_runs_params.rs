#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct WorkflowRunsParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub branch: Option<String>,
    pub event: Option<String>,
    pub status: Option<String>,
    pub workflow_id: Option<String>,
}
