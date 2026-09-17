#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraIssueKeyParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
}
