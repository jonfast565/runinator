#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct CheckRunParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub check_run_id: String,
}
