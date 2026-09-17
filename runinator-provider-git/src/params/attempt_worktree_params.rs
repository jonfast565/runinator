#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct AttemptWorktreeParams {
    pub repo: Option<String>,
    pub branch: String,
    pub path: Option<String>,
    pub base_ref: Option<String>,
}
