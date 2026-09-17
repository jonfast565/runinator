#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraSearchParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub jql: String,
}
