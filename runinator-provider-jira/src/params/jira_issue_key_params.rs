#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraIssueKeyParams {
    pub key: String,
}
