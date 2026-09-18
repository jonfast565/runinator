mod errors;
mod helpers;
mod params;

use std::sync::Arc;
use std::time::Duration;

use runinator_github::{GitHubClient, GitHubOperation};
use runinator_models::{
    errors::SendableError,
    orchestration::DeliverySemantics,
    providers::{
        ActionAuthenticationAlternative, ActionAuthenticationMetadata, ActionMetadata,
        ExecutionProfileSupport, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use runinator_provider_support::polling::{PollPage, poll_pages};
use serde_json::{Value, json};

use helpers::{
    auth_param, checks_summary_response, exact_revision_checks_summary_response, first_pull_number,
    json_response, json_results, parse_params, pull_request_results, repo_owner_param, repo_param,
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
            ]
            .into_iter()
            .map(with_github_authentication)
            .collect(),
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["github".into()],
                contract: None,
                execution_profile: ExecutionProfileSupport::Subprocess,
            },
        }
    }

    fn execute_service(
        &self,
        mut request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        _token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let supplied_token = request
            .parameters
            .get("token")
            .and_then(runinator_models::value::Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        match (supplied_token, request.execution_profile.as_ref()) {
            (true, Some(_)) => return Err(errors::CONFLICTING_AUTHENTICATION.bare()),
            (false, None) => return Err(errors::MISSING_AUTHENTICATION.bare()),
            (false, Some(profile)) => {
                let token = runinator_provider_github_cli::resolve_auth_token(
                    profile,
                    Duration::from_secs(request.timeout_secs.max(1) as u64),
                )?;
                let mut parameters = serde_json::to_value(&request.parameters)?;
                let object = parameters.as_object_mut().ok_or_else(|| {
                    errors::INVALID_PARAMS.error("github parameters must be an object")
                })?;
                object.insert("token".into(), Value::String(token));
                request.parameters = serde_json::from_value(parameters)?;
            }
            (true, None) => {}
        }
        let function = request.action_function.as_str();
        let token = request
            .parameters
            .get("token")
            .and_then(runinator_models::value::Value::as_str)
            .unwrap_or_default();
        let client = GitHubClient::http(
            token,
            Duration::from_secs(request.timeout_secs.max(1) as u64),
        )
        .map_err(github_error)?;
        let response = match function {
            "create_or_update_pr" | "create_pr" | "ensure_pr" => {
                let p: CreatePrParams = parse_params(&request)?;
                let repository = format!("{}/{}", p.base.owner, p.base.repo);
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
                let existing = github_call(
                    &client,
                    GitHubOperation::PullRequests {
                        repository: repository.clone(),
                        state: "open".into(),
                        head: Some(head),
                        per_page: 100,
                        page: 1,
                    },
                )?;
                if let Some(number) = first_pull_number(&existing) {
                    github_call(
                        &client,
                        GitHubOperation::UpdatePull {
                            repository,
                            number,
                            title: p.title,
                            base: p.base_branch.unwrap_or_else(|| "main".into()),
                            body,
                        },
                    )?
                } else {
                    github_call(
                        &client,
                        GitHubOperation::CreatePull {
                            repository,
                            title: p.title,
                            head: p.head,
                            base: p.base_branch.unwrap_or_else(|| "main".into()),
                            body,
                        },
                    )?
                }
            }
            "read_reviews" | "reviews" => {
                let p: PrNumberParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::Reviews {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        pull_number: p.pull_number,
                    },
                )?
            }
            "merge_pull_request" | "merge_pr" => {
                let p: MergePrParams = parse_params(&request)?;
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
                github_call(
                    &client,
                    GitHubOperation::MergePull {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        pull_number: p.pull_number,
                        body: Value::Object(body),
                    },
                )?
            }
            "read_issue_comments" | "comments" => {
                let p: IssueNumberParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::IssueComments {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        issue_number: p.issue_number,
                        per_page: 100,
                        page: 1,
                    },
                )?
            }
            "add_comment" => {
                let p: AddCommentParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::AddComment {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        issue_number: p.issue_number,
                        body: p.body,
                    },
                )?
            }
            "ensure_comment" => {
                let p: EnsureCommentParams = parse_params(&request)?;
                let operation_key = p
                    .operation_key
                    .or_else(|| request.idempotency_key.clone())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| errors::MISSING_OPERATION_KEY.bare())?;
                let marker = format!("<!-- runinator-operation:{operation_key} -->");
                let repository = format!("{}/{}", p.base.owner, p.base.repo);
                let issue_number = p.issue_number.clone();
                let comments = poll_pages(
                    Some(1u32),
                    usize::MAX,
                    |page| {
                        let page = page.unwrap_or(1);
                        let value = github_call(
                            &client,
                            GitHubOperation::IssueComments {
                                repository: repository.clone(),
                                issue_number: issue_number.clone(),
                                per_page: 100,
                                page,
                            },
                        )?;
                        let items = value.as_array().cloned().unwrap_or_default();
                        let next_cursor = (items.len() == 100).then_some(page + 1);
                        Ok::<_, SendableError>(PollPage { items, next_cursor })
                    },
                    |limit| {
                        errors::HTTP_ERROR.error(format!("comment pagination failed: {limit:?}"))
                    },
                )?;
                if let Some(comment) = comments.iter().find(|comment| {
                    comment
                        .get("body")
                        .and_then(Value::as_str)
                        .is_some_and(|body| body.contains(&marker))
                }) {
                    return Ok(TaskExecutionResult { message: Some("github comment already existed".into()), output_json: Some(json!({ "created": false, "operation_key": operation_key, "comment": comment }).into()), chunks: Vec::new(), artifacts: Vec::new() });
                }
                let comment = github_call(
                    &client,
                    GitHubOperation::AddComment {
                        repository,
                        issue_number: p.issue_number,
                        body: format!("{}\n\n{}", p.body, marker),
                    },
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
                github_call(
                    &client,
                    GitHubOperation::RequestReviewers {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        pull_number: p.pull_number,
                        reviewers: p.reviewers,
                        team_reviewers: p.team_reviewers,
                    },
                )?
            }
            "add_assignees" => {
                let p: AddAssigneesParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::AddAssignees {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        issue_number: p.issue_number,
                        assignees: p.assignees,
                    },
                )?
            }
            "read_checks" | "checks" => {
                let p: RefParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::CheckRuns {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        git_ref: p.git_ref,
                        per_page: None,
                        page: None,
                    },
                )?
            }
            "checks_summary" => {
                let p: RefParams = parse_params(&request)?;
                let response = github_call(
                    &client,
                    GitHubOperation::CheckRuns {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        git_ref: p.git_ref,
                        per_page: None,
                        page: None,
                    },
                )?;
                return checks_summary_response(response);
            }
            "exact_revision_check_summary" => {
                let p: ExactRevisionParams = parse_params(&request)?;
                let response = github_call(
                    &client,
                    GitHubOperation::CheckRuns {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        git_ref: p.revision.clone(),
                        per_page: Some(100),
                        page: None,
                    },
                )?;
                return exact_revision_checks_summary_response(response, &p.revision);
            }
            "dispatch_workflow" | "dispatch" => {
                let p: DispatchParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::DispatchWorkflow {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        workflow_id: p.workflow_id,
                        git_ref: p.git_ref,
                        inputs: p.inputs.unwrap_or_else(|| json!({})),
                    },
                )?
            }
            "poll_workflow_runs" | "workflow_runs" => {
                let p: WorkflowRunsParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::WorkflowRuns {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        workflow_id: p.workflow_id,
                        branch: p.branch,
                        event: p.event,
                        status: p.status,
                        per_page: None,
                        page: None,
                    },
                )?
            }
            "rerun_workflow" => {
                let p: WorkflowRunParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::RerunWorkflow {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        run_id: p.run_id,
                    },
                )?
            }
            "rerequest_check" => {
                let p: CheckRunParams = parse_params(&request)?;
                github_call(
                    &client,
                    GitHubOperation::RerequestCheck {
                        repository: format!("{}/{}", p.base.owner, p.base.repo),
                        check_run_id: p.check_run_id,
                    },
                )?
            }
            other => {
                return Err(errors::UNSUPPORTED_ACTION.error(other));
            }
        };
        json_response(response)
    }
}

fn github_error(error: runinator_github::errors::GitHubError) -> SendableError {
    error.descriptor().error(error.detail())
}

fn github_call(client: &GitHubClient, operation: GitHubOperation) -> Result<Value, SendableError> {
    client.execute(operation).map_err(github_error)
}

fn with_github_authentication(mut action: ActionMetadata) -> ActionMetadata {
    action.authentication = Some(ActionAuthenticationMetadata::required(vec![
        ActionAuthenticationAlternative::secrets(["token"]),
        ActionAuthenticationAlternative::ExecutionProfile,
    ]));
    action
}

#[cfg(test)]
mod tests;
