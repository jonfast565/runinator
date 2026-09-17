#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct RequestReviewersParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub pull_number: String,
    #[serde(default)]
    pub reviewers: Vec<String>,
    #[serde(default)]
    pub team_reviewers: Vec<String>,
}
