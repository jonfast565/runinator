use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::ProviderExecutionRequest,
};
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
pub(crate) struct JiraTransitionParams {
    #[serde(flatten)]
    pub base: JiraBaseParams,
    pub key: String,
    pub transition_id: String,
}

pub(crate) fn parse_params<T: serde::de::DeserializeOwned>(
    request: &ProviderExecutionRequest,
) -> Result<T, SendableError> {
    serde_json::from_value(request.parameters.clone().into()).map_err(|e| {
        Box::new(RuntimeError::new(
            "jira.invalid_params".into(),
            e.to_string(),
        )) as SendableError
    })
}
