#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct PromoteRevisionParams {
    pub workspace: Option<String>,
    pub repo: Option<String>,
    pub candidate_sha: String,
    pub target_ref: String,
    pub expected_target_sha: Option<String>,
    pub remote: Option<String>,
    pub push: Option<bool>,
}
