#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct EnsureCommentParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub issue_number: String,
    pub body: String,
    pub operation_key: Option<String>,
}
