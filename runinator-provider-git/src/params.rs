use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct WorktreeParams {
    pub repo: Option<String>,
    pub branch: String,
    pub path: String,
}

#[derive(Deserialize)]
pub(crate) struct WorkspaceParams {
    pub workspace: Option<String>,
    pub repo: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CommitParams {
    pub workspace: Option<String>,
    pub message: String,
}

#[derive(Deserialize)]
pub(crate) struct CleanupParams {
    pub repo: Option<String>,
    pub path: String,
}

#[derive(Deserialize)]
pub(crate) struct PushParams {
    pub workspace: Option<String>,
    pub remote: Option<String>,
    pub branch: String,
    pub set_upstream: Option<bool>,
}

runinator_provider_support::provider_parse_params!(crate::errors::INVALID_PARAMS);
