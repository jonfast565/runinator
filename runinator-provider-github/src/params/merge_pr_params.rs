#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct MergePrParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub pull_number: String,
    pub merge_method: Option<String>,
    pub commit_title: Option<String>,
    pub commit_message: Option<String>,
    pub sha: Option<String>,
}
