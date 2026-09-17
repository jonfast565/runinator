#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraEnsureCommentParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub body: String,
    pub operation_key: Option<String>,
}
