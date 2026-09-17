#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraCommentParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub body: String,
}
