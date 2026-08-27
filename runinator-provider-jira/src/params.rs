use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct JiraBaseParams {
    pub base_url: String,
    pub token: String,
    pub email: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct JiraSearchParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub jql: String,
}

#[derive(Deserialize)]
pub(crate) struct JiraIssueKeyParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
}

#[derive(Deserialize)]
pub(crate) struct JiraCommentParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub body: String,
}

#[derive(Deserialize)]
pub(crate) struct JiraEnsureCommentParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub body: String,
    pub operation_key: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct JiraTransitionParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub transition_id: String,
}

#[derive(Deserialize)]
pub(crate) struct JiraEnsureTransitionParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub transition_id: String,
    pub target_status: String,
    pub operation_key: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct JiraCommentsParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    // optional directory to also write downloaded images into (e.g. a worktree the
    // ai step reads from); images are always registered as run artifacts too.
    pub download_dir: Option<String>,
}

runinator_provider_support::provider_parse_params!(crate::error::INVALID_PARAMS);
