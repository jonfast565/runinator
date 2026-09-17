#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct RefParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    #[serde(rename = "ref")]
    pub git_ref: String,
}
