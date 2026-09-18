#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraEnsureCommentParams {
    pub key: String,
    pub body: String,
    pub operation_key: Option<String>,
}
