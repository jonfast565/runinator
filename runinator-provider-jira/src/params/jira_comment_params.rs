#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraCommentParams {
    pub key: String,
    pub body: String,
}
