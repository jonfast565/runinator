mod errors;
mod helpers;
mod params;

use std::sync::Arc;
use std::time::Duration;

use runinator_models::{
    errors::SendableError,
    orchestration::DeliverySemantics,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::{Value, json};

use helpers::{
    auth_param, checks_summary_response, exact_revision_checks_summary_response, first_pull_number,
    json_response, json_results, parse_params, pull_request_results, repo_owner_param, repo_param,
    response_json,
};
use params::{
    AddAssigneesParams, AddCommentParams, CheckRunParams, CreatePrParams, DispatchParams,
    EnsureCommentParams, ExactRevisionParams, IssueNumberParams, MergePrParams, PrNumberParams,
    RefParams, RequestReviewersParams, WorkflowRunParams, WorkflowRunsParams,
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
                ActionMetadata::new("create_pr", "Create or update a pull request by head")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("title", RuninatorType::String),
                        ParameterMetadata::required("head", RuninatorType::String),
                        ParameterMetadata::optional("base", RuninatorType::String)
                            .with_default(json!("main")),
                        ParameterMetadata::optional("body", RuninatorType::String),
                        ParameterMetadata::optional("operation_key", RuninatorType::String),
                    ])
                    .with_results(pull_request_results())
                    .with_delivery_semantics(DeliverySemantics::Reconcilable),
                ActionMetadata::new(
                    "ensure_pr",
                    "Ensure one open pull request exists for a head",
                )
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
                .with_results(pull_request_results())
                .with_delivery_semantics(DeliverySemantics::Reconcilable),
                ActionMetadata::new("reviews", "Read pull request reviews")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("pull_number", RuninatorType::String),
                    ])
                    .with_results(json_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new("merge_pr", "Merge a pull request")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("pull_number", RuninatorType::String),
                        ParameterMetadata::optional(
                            "merge_method",
                            RuninatorType::Enum(vec![
                                json!("merge").into(),
                                json!("squash").into(),
                                json!("rebase").into(),
                            ]),
                        )
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
                    .with_results(json_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new("add_comment", "Add a comment to an issue or pull request")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("issue_number", RuninatorType::String),
                        ParameterMetadata::required("body", RuninatorType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new(
                    "ensure_comment",
                    "Ensure a provenance-marked comment exists on an issue or pull request",
                )
                .with_parameters(vec![
                    auth_param(),
                    repo_owner_param(),
                    repo_param(),
                    ParameterMetadata::required("issue_number", RuninatorType::String),
                    ParameterMetadata::required("body", RuninatorType::String),
                    ParameterMetadata::optional("operation_key", RuninatorType::String),
                ])
                .with_results(vec![
                    ResultMetadata::new("created", RuninatorType::Boolean),
                    ResultMetadata::new("operation_key", RuninatorType::String),
                    ResultMetadata::new("comment", RuninatorType::Any),
                ])
                .with_delivery_semantics(DeliverySemantics::Reconcilable),
                ActionMetadata::new("request_reviewers", "Request reviewers on a pull request")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("pull_number", RuninatorType::String),
                        ParameterMetadata::optional(
                            "reviewers",
                            RuninatorType::array(RuninatorType::String),
                        ),
                        ParameterMetadata::optional(
                            "team_reviewers",
                            RuninatorType::array(RuninatorType::String),
                        ),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("add_assignees", "Add assignees to an issue or pull request")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("issue_number", RuninatorType::String),
                        ParameterMetadata::required(
                            "assignees",
                            RuninatorType::array(RuninatorType::String),
                        ),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new("checks", "Read check runs for a reference")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("ref", RuninatorType::String),
                    ])
                    .with_results(json_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
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
                    ])
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new(
                    "exact_revision_check_summary",
                    "Summarize check runs and reject results for any other revision",
                )
                .with_parameters(vec![
                    auth_param(),
                    repo_owner_param(),
                    repo_param(),
                    ParameterMetadata::required("revision", RuninatorType::String),
                ])
                .with_results(vec![
                    ResultMetadata::new("revision", RuninatorType::String),
                    ResultMetadata::new("status", RuninatorType::String),
                    ResultMetadata::new("passed", RuninatorType::Integer),
                    ResultMetadata::new("pending", RuninatorType::Integer),
                    ResultMetadata::new("failed", RuninatorType::Integer),
                    ResultMetadata::new("total", RuninatorType::Integer),
                    ResultMetadata::new("raw", RuninatorType::Any),
                ])
                .with_delivery_semantics(DeliverySemantics::Idempotent),
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
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::optional("branch", RuninatorType::String),
                        ParameterMetadata::optional("event", RuninatorType::String),
                        ParameterMetadata::optional("status", RuninatorType::String),
                        ParameterMetadata::optional("workflow_id", RuninatorType::String),
                    ])
                    .with_results(json_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new("rerun_workflow", "Rerun a GitHub Actions workflow run")
                    .with_parameters(vec![
                        auth_param(),
                        repo_owner_param(),
                        repo_param(),
                        ParameterMetadata::required("run_id", RuninatorType::String),
                    ])
                    .with_results(json_results()),
                ActionMetadata::new(
                    "rerequest_check",
                    "Request that a GitHub check run execute again",
                )
                .with_parameters(vec![
                    auth_param(),
                    repo_owner_param(),
                    repo_param(),
                    ParameterMetadata::required("check_run_id", RuninatorType::String),
                ])
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
            "create_or_update_pr" | "create_pr" | "ensure_pr" => {
                let p: CreatePrParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                let body = if function == "ensure_pr" {
                    let operation_key = p
                        .operation_key
                        .or_else(|| request.idempotency_key.clone())
                        .filter(|key| !key.trim().is_empty())
                        .ok_or_else(|| errors::MISSING_OPERATION_KEY.bare())?;
                    let marker = format!("<!-- runinator-operation:{operation_key} -->");
                    let body = p.body.as_deref().unwrap_or_default();
                    if body.contains(&marker) {
                        body.to_string()
                    } else if body.is_empty() {
                        marker
                    } else {
                        format!("{body}\n\n{marker}")
                    }
                } else {
                    p.body.unwrap_or_default()
                };
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
                            "body": body
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
                            "body": body
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
            "add_comment" => {
                let p: AddCommentParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .post(format!(
                        "{api}/repos/{}/{}/issues/{}/comments",
                        p.base.owner, p.base.repo, p.issue_number
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .json(&json!({ "body": p.body }))
                    .send()?
            }
            "ensure_comment" => {
                let p: EnsureCommentParams = parse_params(&request)?;
                let operation_key = p
                    .operation_key
                    .or_else(|| request.idempotency_key.clone())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| errors::MISSING_OPERATION_KEY.bare())?;
                let marker = format!("<!-- runinator-operation:{operation_key} -->");
                let auth = format!("Bearer {}", p.base.token);
                let comments_url = format!(
                    "{api}/repos/{}/{}/issues/{}/comments",
                    p.base.owner, p.base.repo, p.issue_number
                );
                let mut page = 1u32;
                loop {
                    let existing = response_json(
                        client
                            .get(&comments_url)
                            .header("Authorization", &auth)
                            .header("Accept", "application/vnd.github+json")
                            .query(&[("per_page", "100"), ("page", &page.to_string())])
                            .send()?,
                    )?;
                    let comments = existing.as_array().cloned().unwrap_or_default();
                    if let Some(comment) = comments.iter().find(|comment| {
                        comment
                            .get("body")
                            .and_then(Value::as_str)
                            .is_some_and(|body| body.contains(&marker))
                    }) {
                        return Ok(TaskExecutionResult {
                            message: Some("github comment already existed".into()),
                            output_json: Some(
                                json!({
                                    "created": false,
                                    "operation_key": operation_key,
                                    "comment": comment
                                })
                                .into(),
                            ),
                            chunks: Vec::new(),
                            artifacts: Vec::new(),
                        });
                    }
                    if comments.len() < 100 {
                        break;
                    }
                    page += 1;
                }
                let comment = response_json(
                    client
                        .post(comments_url)
                        .header("Authorization", &auth)
                        .header("Accept", "application/vnd.github+json")
                        .json(&json!({ "body": format!("{}\n\n{}", p.body, marker) }))
                        .send()?,
                )?;
                return Ok(TaskExecutionResult {
                    message: Some("github comment created".into()),
                    output_json: Some(
                        json!({
                            "created": true,
                            "operation_key": operation_key,
                            "comment": comment
                        })
                        .into(),
                    ),
                    chunks: Vec::new(),
                    artifacts: Vec::new(),
                });
            }
            "request_reviewers" => {
                let p: RequestReviewersParams = parse_params(&request)?;
                if p.reviewers.is_empty() && p.team_reviewers.is_empty() {
                    return Err(errors::MISSING_REVIEWERS.bare());
                }
                let auth = format!("Bearer {}", p.base.token);
                client
                    .post(format!(
                        "{api}/repos/{}/{}/pulls/{}/requested_reviewers",
                        p.base.owner, p.base.repo, p.pull_number
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .json(&json!({
                        "reviewers": p.reviewers,
                        "team_reviewers": p.team_reviewers
                    }))
                    .send()?
            }
            "add_assignees" => {
                let p: AddAssigneesParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .post(format!(
                        "{api}/repos/{}/{}/issues/{}/assignees",
                        p.base.owner, p.base.repo, p.issue_number
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .json(&json!({ "assignees": p.assignees }))
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
            "exact_revision_check_summary" => {
                let p: ExactRevisionParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                let response = client
                    .get(format!(
                        "{api}/repos/{}/{}/commits/{}/check-runs",
                        p.base.owner, p.base.repo, p.revision
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .query(&[("per_page", "100")])
                    .send()?;
                return exact_revision_checks_summary_response(response, &p.revision);
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
                let p: WorkflowRunsParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                let base_url = match &p.workflow_id {
                    Some(workflow_id) => format!(
                        "{api}/repos/{}/{}/actions/workflows/{}/runs",
                        p.base.owner, p.base.repo, workflow_id
                    ),
                    None => format!("{api}/repos/{}/{}/actions/runs", p.base.owner, p.base.repo),
                };
                let mut filters: Vec<(&str, String)> = Vec::new();
                if let Some(branch) = p.branch {
                    filters.push(("branch", branch));
                }
                if let Some(event) = p.event {
                    filters.push(("event", event));
                }
                if let Some(status) = p.status {
                    filters.push(("status", status));
                }
                let url = reqwest::Url::parse_with_params(&base_url, &filters)?;
                client
                    .get(url)
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            "rerun_workflow" => {
                let p: WorkflowRunParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .post(format!(
                        "{api}/repos/{}/{}/actions/runs/{}/rerun",
                        p.base.owner, p.base.repo, p.run_id
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            "rerequest_check" => {
                let p: CheckRunParams = parse_params(&request)?;
                let auth = format!("Bearer {}", p.base.token);
                client
                    .post(format!(
                        "{api}/repos/{}/{}/check-runs/{}/rerequest",
                        p.base.owner, p.base.repo, p.check_run_id
                    ))
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
