use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub(crate) struct GitHubBaseParams {
    pub token: String,
    pub owner: String,
    pub repo: String,
}

#[derive(Deserialize)]
pub(crate) struct CreatePrParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub title: String,
    pub head: String,
    #[serde(alias = "base")]
    pub base_branch: Option<String>,
    pub body: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PrNumberParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub pull_number: String,
}

#[derive(Deserialize)]
pub(crate) struct MergePrParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub pull_number: String,
    pub merge_method: Option<String>,
    pub commit_title: Option<String>,
    pub commit_message: Option<String>,
    pub sha: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct IssueNumberParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub issue_number: String,
}

#[derive(Deserialize)]
pub(crate) struct RefParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    #[serde(rename = "ref")]
    pub git_ref: String,
}

#[derive(Deserialize)]
pub(crate) struct AddCommentParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub issue_number: String,
    pub body: String,
}

#[derive(Deserialize)]
pub(crate) struct RequestReviewersParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub pull_number: String,
    #[serde(default)]
    pub reviewers: Vec<String>,
    #[serde(default)]
    pub team_reviewers: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct AddAssigneesParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub issue_number: String,
    pub assignees: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct DispatchParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub workflow_id: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub inputs: Option<Value>,
}

#[derive(Deserialize)]
pub(crate) struct WorkflowRunsParams {
    #[serde(flatten)]
    pub base: GitHubBaseParams,
    pub branch: Option<String>,
    pub event: Option<String>,
    pub status: Option<String>,
    pub workflow_id: Option<String>,
}
