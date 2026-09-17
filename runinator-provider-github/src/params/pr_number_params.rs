#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct PrNumberParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub pull_number: String,
}
