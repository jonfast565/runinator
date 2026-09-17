#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraBaseParams {
    pub base_url: String,
    pub token: String,
    pub email: Option<String>,
}
