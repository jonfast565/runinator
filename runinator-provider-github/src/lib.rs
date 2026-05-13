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
use serde_json::{Value, json};

#[derive(Deserialize)]
struct GitHubBaseParams {
    token: String,
    owner: String,
    repo: String,
}

#[derive(Deserialize)]
struct CreatePrParams {
    #[serde(flatten)]
    base: GitHubBaseParams,
    title: String,
    head: String,
    base_branch: Option<String>,
    body: Option<String>,
}

#[derive(Deserialize)]
struct PrNumberParams {
    #[serde(flatten)]
    base: GitHubBaseParams,
    pull_number: String,
}

#[derive(Deserialize)]
struct MergePrParams {
    #[serde(flatten)]
    base: GitHubBaseParams,
    pull_number: String,
    merge_method: Option<String>,
    commit_title: Option<String>,
    commit_message: Option<String>,
    sha: Option<String>,
}

#[derive(Deserialize)]
struct IssueNumberParams {
    #[serde(flatten)]
    base: GitHubBaseParams,
    issue_number: String,
}

#[derive(Deserialize)]
struct RefParams {
    #[serde(flatten)]
    base: GitHubBaseParams,
    #[serde(rename = "ref")]
    git_ref: String,
}

#[derive(Deserialize)]
struct DispatchParams {
    #[serde(flatten)]
    base: GitHubBaseParams,
    workflow_id: String,
    #[serde(rename = "ref")]
    git_ref: String,
    inputs: Option<Value>,
}

#[derive(Clone)]
pub struct GitHubProvider;

impl Provider for GitHubProvider {
    fn name(&self) -> String {
        "github".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("create_pr", "Create a new pull request")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("title", ParameterValueType::String),
                        ParameterMetadata::required("head", ParameterValueType::String),
                        ParameterMetadata::optional("base", ParameterValueType::String)
                            .with_default(json!("main")),
                        ParameterMetadata::optional("body", ParameterValueType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("reviews", "Read pull request reviews")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("pull_number", ParameterValueType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("merge_pr", "Merge a pull request")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("pull_number", ParameterValueType::String),
                        ParameterMetadata::optional("merge_method", ParameterValueType::String)
                            .with_default(json!("squash")),
                        ParameterMetadata::optional("commit_title", ParameterValueType::String),
                        ParameterMetadata::optional("commit_message", ParameterValueType::String),
                        ParameterMetadata::optional("sha", ParameterValueType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("comments", "Read issue or PR comments")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("issue_number", ParameterValueType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("checks", "Read check runs for a reference")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("ref", ParameterValueType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("dispatch", "Dispatch a workflow run")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("workflow_id", ParameterValueType::String),
                        ParameterMetadata::required("ref", ParameterValueType::String),
                        ParameterMetadata::optional("inputs", ParameterValueType::Object),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("workflow_runs", "List actions workflow runs")
                    .with_parameters(vec![auth_param(), repo_owner_param(), repo_param()])
                    .with_results(json_results()),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["github".into()],
                contract: None,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let function = request.action_function.as_str();
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(request.timeout_secs.max(1) as u64))
            .user_agent("runinator")
            .build()?;
        let api = "https://api.github.com";
        let response = match function {
            "create_or_update_pr" | "create_pr" => {
                let p: CreatePrParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .post(format!(
                        "{api}/repos/{}/{}/pulls",
                        p.base.owner, p.base.repo
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .json(&json!({
                        "title": p.title,
                        "head": p.head,
                        "base": p.base_branch.as_deref().unwrap_or("main"),
                        "body": p.body.as_deref().unwrap_or("")
                    }))
                    .send()?
            }
            "read_reviews" | "reviews" => {
                let p: PrNumberParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .get(format!(
                        "{api}/repos/{}/{}/pulls/{}/reviews",
                        p.base.owner, p.base.repo, p.pull_number
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            "merge_pull_request" | "merge_pr" => {
                let p: MergePrParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                let mut body = serde_json::Map::new();
                body.insert(
                    "merge_method".into(),
                    json!(p.merge_method.as_deref().unwrap_or("squash")),
                );
                if let Some(commit_title) = p.commit_title {
                    body.insert("commit_title".into(), json!(commit_title));
                }
                if let Some(commit_message) = p.commit_message {
                    body.insert("commit_message".into(), json!(commit_message));
                }
                if let Some(sha) = p.sha {
                    body.insert("sha".into(), json!(sha));
                }
                client
                    .put(format!(
                        "{api}/repos/{}/{}/pulls/{}/merge",
                        p.base.owner, p.base.repo, p.pull_number
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .json(&Value::Object(body))
                    .send()?
            }
            "read_issue_comments" | "comments" => {
                let p: IssueNumberParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .get(format!(
                        "{api}/repos/{}/{}/issues/{}/comments",
                        p.base.owner, p.base.repo, p.issue_number
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            "read_checks" | "checks" => {
                let p: RefParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .get(format!(
                        "{api}/repos/{}/{}/commits/{}/check-runs",
                        p.base.owner, p.base.repo, p.git_ref
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            "dispatch_workflow" | "dispatch" => {
                let p: DispatchParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .post(format!(
                        "{api}/repos/{}/{}/actions/workflows/{}/dispatches",
                        p.base.owner, p.base.repo, p.workflow_id
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .json(&json!({
                        "ref": p.git_ref,
                        "inputs": p.inputs.unwrap_or_else(|| json!({}))
                    }))
                    .send()?
            }
            "poll_workflow_runs" | "workflow_runs" => {
                let p: GitHubBaseParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.token);
                client
                    .get(format!("{api}/repos/{}/{}/actions/runs", p.owner, p.repo))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            other => {
                return Err(Box::new(RuntimeError::new(
                    "github.unsupported_action".into(),
                    format!("Unsupported GitHub action {other}"),
                )));
            }
        };
        json_response("github", response)
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(
    request: &ProviderExecutionRequest,
) -> Result<T, SendableError> {
    serde_json::from_value(request.parameters.clone()).map_err(|e| {
        Box::new(RuntimeError::new(
            "github.invalid_params".into(),
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

fn auth_param() -> ParameterMetadata {
    ParameterMetadata::required("token", ParameterValueType::String).secret()
}

fn repo_owner_param() -> ParameterMetadata {
    ParameterMetadata::required("owner", ParameterValueType::String)
}

fn repo_param() -> ParameterMetadata {
    ParameterMetadata::required("repo", ParameterValueType::String)
}

fn json_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("response", ParameterValueType::Json)
            .with_description("Raw GitHub API response body."),
    ]
}

#[cfg(test)]
mod tests;
