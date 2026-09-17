#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct CleanupParams {
    pub repo: Option<String>,
    pub path: String,
}
