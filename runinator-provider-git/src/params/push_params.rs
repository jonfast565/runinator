#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct PushParams {
    pub workspace: Option<String>,
    pub remote: Option<String>,
    pub branch: String,
    pub set_upstream: Option<bool>,
}
