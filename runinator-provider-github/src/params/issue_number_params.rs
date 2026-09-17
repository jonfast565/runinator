#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct IssueNumberParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub issue_number: String,
}
