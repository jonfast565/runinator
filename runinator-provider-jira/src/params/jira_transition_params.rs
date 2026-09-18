#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct JiraTransitionParams {
    pub key: String,
    pub transition_id: String,
}
