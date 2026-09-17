#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraTransitionParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub transition_id: String,
}
