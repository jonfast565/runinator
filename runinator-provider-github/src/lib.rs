mod errors;
mod helpers;
mod params;

use std::sync::Arc;
use std::time::Duration;

use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::{Value, json};

use helpers::{
    auth_param, checks_summary_response, first_pull_number, json_response, json_results,
    parse_params, pull_request_results, repo_owner_param, repo_param,
};
use params::{
    CreatePrParams, DispatchParams, GitHubBaseParams, IssueNumberParams, MergePrParams,
    PrNumberParams, RefParams,
};

#[cfg(test)]
pub(crate) use helpers::summarize_check_runs;

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
                        ParameterMetadata::required("title", RuninatorType::String),
                        ParameterMetadata::required("head", RuninatorType::String),
                        ParameterMetadata::optional("base", RuninatorType::String)
                            .with_default(json!("main")),
                        ParameterMetadata::optional("body", RuninatorType::String),
                    ])
                    .with_results(pull_request_results()),
                ActionMetadata::new("reviews", "Read pull request reviews")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("pull_number", RuninatorType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("merge_pr", "Merge a pull request")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("pull_number", RuninatorType::String),
                        ParameterMetadata::optional("merge_method", RuninatorType::String)
                            .with_default(json!("squash")),
                        ParameterMetadata::optional("commit_title", RuninatorType::String),
                        ParameterMetadata::optional("commit_message", RuninatorType::String),
                        ParameterMetadata::optional("sha", RuninatorType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("comments", "Read issue or PR comments")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("issue_number", RuninatorType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("checks", "Read check runs for a reference")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("ref", RuninatorType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("checks_summary", "Summarize check runs for a reference")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("ref", RuninatorType::String),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("status", RuninatorType::String),
                        ResultMetadata::new("passed", RuninatorType::Integer),
                        ResultMetadata::new("pending", RuninatorType::Integer),
                        ResultMetadata::new("failed", RuninatorType::Integer),
                        ResultMetadata::new("total", RuninatorType::Integer),
                        ResultMetadata::new("raw", RuninatorType::Any),
                    ]),
                ActionMetadata::new("dispatch", "Dispatch a workflow run")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("workflow_id", RuninatorType::String),
                        ParameterMetadata::required("ref", RuninatorType::String),
                        ParameterMetadata::optional(
                            "inputs",
                            RuninatorType::map(RuninatorType::String),
                        ),
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
        _token: runinator_plugin::cancel::CancellationToken,
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
                let head = if p.head.contains(':') {
                    p.head.clone()
                } else {
                    format!("{}:{}", p.base.owner, p.head)
                };
                let pulls_url = reqwest::Url::parse_with_params(
                    &format!("{api}/repos/{}/{}/pulls", p.base.owner, p.base.repo),
                    &[("state", "open"), ("head", head.as_str())],
                )?;
                let existing = client
                    .get(pulls_url)
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?;
                if !existing.status().is_success() {
                    existing
                } else if let Some(number) = first_pull_number(existing)? {
                    client
                        .patch(format!(
                            "{api}/repos/{}/{}/pulls/{number}",
                            p.base.owner, p.base.repo
                        ))
                        .header("Authorization", &auth)
                        .header("Accept", "application/vnd.github+json")
                        .json(&json!({
                            "title": p.title,
                            "base": p.base_branch.as_deref().unwrap_or("main"),
                            "body": p.body.as_deref().unwrap_or("")
                        }))
                        .send()?
                } else {
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
            "checks_summary" => {
                let p: RefParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                let response = client
                    .get(format!(
                        "{api}/repos/{}/{}/commits/{}/check-runs",
                        p.base.owner, p.base.repo, p.git_ref
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?;
                return checks_summary_response(response);
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
                return Err(errors::UNSUPPORTED_ACTION.error(other));
            }
        };
        json_response(response)
    }
}

#[cfg(test)]
mod tests;
