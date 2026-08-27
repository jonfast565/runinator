use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct WorktreeParams {
    pub repo: Option<String>,
    pub branch: String,
    pub path: String,
}

#[derive(Deserialize)]
pub(crate) struct AttemptWorktreeParams {
    pub repo: Option<String>,
    pub branch: String,
    pub path: Option<String>,
    pub base_ref: Option<String>,
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

#[derive(Deserialize)]
pub(crate) struct ArchivePatchParams {
    pub workspace: Option<String>,
    pub name: Option<String>,
}

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

runinator_provider_support::provider_parse_params!(crate::errors::INVALID_PARAMS);
