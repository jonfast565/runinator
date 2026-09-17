#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct CreatePrParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub title: String,
    pub head: String,
    #[serde(alias = "base")]
    pub base_branch: Option<String>,
    pub body: Option<String>,
    pub operation_key: Option<String>,
}
