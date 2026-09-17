#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct ExactRevisionParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub revision: String,
}
