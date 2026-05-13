use std::sync::Arc;
use std::time::Duration;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct JiraBaseParams {
    base_url: String,
    token: String,
    email: Option<String>,
}

#[derive(Deserialize)]
struct JiraSearchParams {
    #[serde(flatten)]
    base: JiraBaseParams,
    jql: String,
}

#[derive(Deserialize)]
struct JiraIssueKeyParams {
    #[serde(flatten)]
    base: JiraBaseParams,
    key: String,
}

#[derive(Deserialize)]
struct JiraCommentParams {
    #[serde(flatten)]
    base: JiraBaseParams,
    key: String,
    body: String,
}

#[derive(Deserialize)]
struct JiraTransitionParams {
    #[serde(flatten)]
    base: JiraBaseParams,
    key: String,
    transition_id: String,
}

#[derive(Clone)]
pub struct JiraProvider;

impl Provider for JiraProvider {
    fn name(&self) -> String {
        "jira".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("search", "Search Jira issues using JQL")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        ParameterMetadata::required("jql", ParameterValueType::String),
                    ])
                    .with_results(jira_results()),
                ActionMetadata::new("fetch", "Fetch a single Jira issue by key")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                    ])
                    .with_results(jira_results()),
                ActionMetadata::new("comment", "Add a comment to a Jira issue")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                        ParameterMetadata::required("body", ParameterValueType::String),
                    ])
                    .with_results(jira_results()),
                ActionMetadata::new("transition", "Transition a Jira issue to a new status")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                        ParameterMetadata::required("transition_id", ParameterValueType::String),
                    ])
                    .with_results(jira_results()),
                ActionMetadata::new("poll", "Poll the status of a Jira issue")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                    ])
                    .with_results(jira_results()),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["jira".into()],
                contract: None,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(request.timeout_secs.max(1) as u64))
            .build()?;
        let function = request.action_function.as_str();
        let response = match function {
            "search_external_items" | "search" => {
                let p: JiraSearchParams = parse_params(&request)?;
                client
                    .get(format!("{}/rest/api/3/search", p.base.base_url))
                    .query(&[("jql", &p.jql)])
                    .basic_auth(
                        p.base.email.as_deref().unwrap_or_default(),
                        Some(&p.base.token),
                    )
                    .send()?
            }
            "fetch_item" | "fetch" => {
                let p: JiraIssueKeyParams = parse_params(&request)?;
                client
                    .get(format!("{}/rest/api/3/issue/{}", p.base.base_url, p.key))
                    .basic_auth(
                        p.base.email.as_deref().unwrap_or_default(),
                        Some(&p.base.token),
                    )
                    .send()?
            }
            "add_comment" | "comment" => {
                let p: JiraCommentParams = parse_params(&request)?;
                client
                    .post(format!("{}/rest/api/3/issue/{}/comment", p.base.base_url, p.key))
                    .basic_auth(p.base.email.as_deref().unwrap_or_default(), Some(&p.base.token))
                    .json(&json!({ "body": { "type": "doc", "version": 1, "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": p.body }] }] } }))
                    .send()?
            }
            "transition_item" | "transition" => {
                let p: JiraTransitionParams = parse_params(&request)?;
                client
                    .post(format!(
                        "{}/rest/api/3/issue/{}/transitions",
                        p.base.base_url, p.key
                    ))
                    .basic_auth(
                        p.base.email.as_deref().unwrap_or_default(),
                        Some(&p.base.token),
                    )
                    .json(&json!({ "transition": { "id": p.transition_id } }))
                    .send()?
            }
            "poll_status" | "poll" => {
                let p: JiraIssueKeyParams = parse_params(&request)?;
                client
                    .get(format!("{}/rest/api/3/issue/{}", p.base.base_url, p.key))
                    .basic_auth(
                        p.base.email.as_deref().unwrap_or_default(),
                        Some(&p.base.token),
                    )
                    .send()?
            }
            other => {
                return Err(Box::new(RuntimeError::new(
                    "jira.unsupported_action".into(),
                    format!("Unsupported Jira action {other}"),
                )));
            }
        };
        json_response("jira", response)
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(
    request: &ProviderExecutionRequest,
) -> Result<T, SendableError> {
    serde_json::from_value(request.parameters.clone()).map_err(|e| {
        Box::new(RuntimeError::new(
            "jira.invalid_params".into(),
            e.to_string(),
        )) as SendableError
    })
}

fn json_response(
    provider: &str,
    response: reqwest::blocking::Response,
) -> Result<TaskExecutionResult, SendableError> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(Box::new(RuntimeError::new(
            format!("{provider}.http_error"),
            format!("HTTP {status}: {text}"),
        )));
    }
    let output = if text.trim().is_empty() {
        json!({ "status": status.as_u16() })
    } else {
        serde_json::from_str(&text)
            .unwrap_or_else(|_| json!({ "body": text, "status": status.as_u16() }))
    };
    Ok(TaskExecutionResult {
        message: Some(format!("{provider} action completed")),
        output_json: Some(output),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn base_param() -> ParameterMetadata {
    ParameterMetadata::required("base_url", ParameterValueType::String)
}

fn token_param() -> ParameterMetadata {
    ParameterMetadata::required("token", ParameterValueType::String).secret()
}

fn email_param() -> ParameterMetadata {
    ParameterMetadata::optional("email", ParameterValueType::String)
}

fn issue_key_param() -> ParameterMetadata {
    ParameterMetadata::required("key", ParameterValueType::String)
}

fn jira_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("response", ParameterValueType::Json)
            .with_description("Raw Jira API response body."),
    ]
}

#[cfg(test)]
mod tests;
