#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct AddAssigneesParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub issue_number: String,
    pub assignees: Vec<String>,
}
