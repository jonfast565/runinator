#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraSearchParams {
    pub jql: String,
}
