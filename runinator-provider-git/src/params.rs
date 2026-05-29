use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::ProviderExecutionRequest,
};
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

pub(crate) fn parse_params<T: serde::de::DeserializeOwned>(
    request: &ProviderExecutionRequest,
) -> Result<T, SendableError> {
    serde_json::from_value(request.parameters.clone().into()).map_err(|e| {
        Box::new(RuntimeError::new(
            "git.invalid_params".into(),
            e.to_string(),
        )) as SendableError
    })
}
