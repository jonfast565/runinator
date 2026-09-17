#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct AddCommentParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub issue_number: String,
    pub body: String,
}
