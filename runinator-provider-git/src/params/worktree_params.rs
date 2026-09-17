#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct WorktreeParams {
    pub repo: Option<String>,
    pub branch: String,
    pub path: String,
}
