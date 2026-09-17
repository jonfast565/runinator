#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct CommitParams {
    pub workspace: Option<String>,
    pub message: String,
}
