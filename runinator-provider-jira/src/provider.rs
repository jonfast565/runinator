use std::sync::Arc;
use std::time::Duration;

use runinator_jira::{JiraClient, JiraCredentials, JiraOperation};
use runinator_models::json;
use runinator_models::{
    errors::SendableError,
    orchestration::DeliverySemantics,
    providers::{
        ActionAuthenticationAlternative, ActionAuthenticationMetadata, ActionMetadata,
        ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};

use crate::comments::{fetch_all_comments, jira_fetch_comments, render_comment_body};
use crate::error::{MISSING_OPERATION_KEY, UNSUPPORTED_ACTION, client_error};
use crate::metadata::{
    base_param, comments_results, email_param, issue_key_param, jira_results, token_param,
};
use crate::params::{
    JiraCommentParams, JiraCommentsParams, JiraEnsureCommentParams, JiraEnsureTransitionParams,
    JiraIssueKeyParams, JiraSearchParams, JiraTransitionParams, parse_params,
};
use crate::response::json_response;
use crate::search::jira_search_all;

#[derive(Clone)]
pub struct JiraProvider;

impl Provider for JiraProvider {
    fn name(&self) -> String {
        "jira".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: {
                let mut actions =
                    vec![
                ActionMetadata::new("search", "Search Jira issues using JQL")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        ParameterMetadata::required("jql", RuninatorType::String),
                    ])
                    .with_results(jira_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new("fetch", "Fetch a single Jira issue by key")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                    ])
                    .with_results(jira_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new("comment", "Add a comment to a Jira issue")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                        ParameterMetadata::required("body", RuninatorType::String),
                    ])
                    .with_results(jira_results()),
                ActionMetadata::new(
                    "ensure_comment",
                    "Ensure a provenance-marked comment exists on a Jira issue",
                )
                .with_parameters(vec![
                    base_param(),
                    token_param(),
                    email_param(),
                    issue_key_param(),
                    ParameterMetadata::required("body", RuninatorType::String),
                    ParameterMetadata::optional("operation_key", RuninatorType::String),
                ])
                .with_results(vec![
                    runinator_models::providers::ResultMetadata::new(
                        "created",
                        RuninatorType::Boolean,
                    ),
                    runinator_models::providers::ResultMetadata::new(
                        "operation_key",
                        RuninatorType::String,
                    ),
                    runinator_models::providers::ResultMetadata::new("comment", RuninatorType::Any),
                ])
                .with_delivery_semantics(DeliverySemantics::Reconcilable),
                ActionMetadata::new(
                    "comments",
                    "Fetch and parse Jira issue comments (with images) for AI",
                )
                .with_parameters(vec![
                    base_param(),
                    token_param(),
                    email_param(),
                    issue_key_param(),
                    ParameterMetadata::optional("download_dir", RuninatorType::String),
                ])
                .with_results(comments_results())
                .with_delivery_semantics(DeliverySemantics::Idempotent),
                ActionMetadata::new("transition", "Transition a Jira issue to a new status")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                        ParameterMetadata::required("transition_id", RuninatorType::String),
                    ])
                    .with_results(jira_results()),
                ActionMetadata::new(
                    "ensure_transition",
                    "Transition a Jira issue only when it has not reached the target status",
                )
                .with_parameters(vec![
                    base_param(),
                    token_param(),
                    email_param(),
                    issue_key_param(),
                    ParameterMetadata::required("transition_id", RuninatorType::String),
                    ParameterMetadata::required("target_status", RuninatorType::String),
                    ParameterMetadata::optional("operation_key", RuninatorType::String),
                ])
                .with_results(vec![
                    runinator_models::providers::ResultMetadata::new(
                        "changed",
                        RuninatorType::Boolean,
                    ),
                    runinator_models::providers::ResultMetadata::new(
                        "operation_key",
                        RuninatorType::String,
                    ),
                    runinator_models::providers::ResultMetadata::new("status", RuninatorType::Any),
                    runinator_models::providers::ResultMetadata::new(
                        "response",
                        RuninatorType::Any,
                    ),
                ])
                .with_delivery_semantics(DeliverySemantics::Reconcilable),
                ActionMetadata::new("poll", "Poll the status of a Jira issue")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                    ])
                    .with_results(jira_results())
                    .with_delivery_semantics(DeliverySemantics::Idempotent),
                ];
                for action in &mut actions {
                    action.authentication = Some(ActionAuthenticationMetadata::required(vec![
                        ActionAuthenticationAlternative::secrets(["token"]),
                    ]));
                }
                actions
            },
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["jira".into()],
                contract: None,
                execution_profile: Default::default(),
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        _token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let base_url = request
            .parameters
            .get("base_url")
            .and_then(runinator_models::value::Value::as_str)
            .unwrap_or_default();
        let token = request
            .parameters
            .get("token")
            .and_then(runinator_models::value::Value::as_str)
            .unwrap_or_default();
        let email = request
            .parameters
            .get("email")
            .and_then(runinator_models::value::Value::as_str)
            .unwrap_or_default();
        let client = JiraClient::new(
            base_url,
            JiraCredentials {
                email: email.into(),
                token: token.into(),
            },
            Duration::from_secs(request.timeout_secs.max(1) as u64),
        )
        .map_err(|error| client_error("jira client build failed", error))?;
        let function = request.action_function.as_str();
        let response = match function {
            "search_external_items" | "search" => {
                let p: JiraSearchParams = parse_params(&request)?;
                return jira_search_all(&client, &p);
            }
            "fetch_item" | "fetch" => {
                let p: JiraIssueKeyParams = parse_params(&request)?;
                jira_call(
                    &client,
                    JiraOperation::Issue {
                        key: p.key,
                        fields: None,
                    },
                    "jira fetch request failed",
                )?
            }
            "add_comment" | "comment" => {
                let p: JiraCommentParams = parse_params(&request)?;
                jira_call(&client, JiraOperation::AddComment { key: p.key, body: json!({ "body": { "type": "doc", "version": 1, "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": p.body }] }] } }).into() }, "jira comment request failed")?
            }
            "ensure_comment" => {
                let p: JiraEnsureCommentParams = parse_params(&request)?;
                let operation_key = p
                    .operation_key
                    .or_else(|| request.idempotency_key.clone())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| MISSING_OPERATION_KEY.bare())?;
                let marker = format!("[runinator-operation:{operation_key}]");
                let comments = fetch_all_comments(&client, &p.key)?;
                if let Some(comment) = comments
                    .iter()
                    .find(|comment| render_comment_body(comment.get("body")).contains(&marker))
                {
                    return Ok(TaskExecutionResult {
                        message: Some("jira comment already existed".into()),
                        output_json: Some(json!({
                            "created": false,
                            "operation_key": operation_key,
                            "comment": comment
                        })),
                        chunks: Vec::new(),
                        artifacts: Vec::new(),
                    });
                }
                let comment = jira_call(
                    &client,
                    JiraOperation::AddComment {
                        key: p.key,
                        body: json!({
                            "body": {
                                "type": "doc",
                                "version": 1,
                                "content": [{
                                    "type": "paragraph",
                                    "content": [{
                                        "type": "text",
                                        "text": format!("{}\n\n{}", p.body, marker)
                                    }]
                                }]
                            }
                        })
                        .into(),
                    },
                    "jira ensure comment request failed",
                )?;
                return Ok(TaskExecutionResult {
                    message: Some("jira comment created".into()),
                    output_json: Some(json!({
                        "created": true,
                        "operation_key": operation_key,
                        "comment": comment
                    })),
                    chunks: Vec::new(),
                    artifacts: Vec::new(),
                });
            }
            "read_comments" | "comments" => {
                let p: JiraCommentsParams = parse_params(&request)?;
                return jira_fetch_comments(&client, &p, &request.artifact_dir);
            }
            "transition_item" | "transition" => {
                let p: JiraTransitionParams = parse_params(&request)?;
                jira_call(
                    &client,
                    JiraOperation::Transition {
                        key: p.key,
                        transition_id: p.transition_id,
                        update: None,
                    },
                    "jira transition request failed",
                )?
            }
            "ensure_transition" => {
                let p: JiraEnsureTransitionParams = parse_params(&request)?;
                let operation_key = p
                    .operation_key
                    .or_else(|| request.idempotency_key.clone())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| MISSING_OPERATION_KEY.bare())?;
                let issue = jira_call(
                    &client,
                    JiraOperation::Issue {
                        key: p.key.clone(),
                        fields: Some("status".into()),
                    },
                    "jira status reconciliation failed",
                )?;
                let status = issue.pointer("/fields/status").cloned().unwrap_or_default();
                let reached_target = status
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| value == p.target_status)
                    || status
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|value| value == p.target_status);
                if reached_target {
                    return Ok(TaskExecutionResult {
                        message: Some("jira issue already reached target status".into()),
                        output_json: Some(json!({
                            "changed": false,
                            "operation_key": operation_key,
                            "status": status,
                            "response": issue
                        })),
                        chunks: Vec::new(),
                        artifacts: Vec::new(),
                    });
                }
                let transition = jira_call(
                    &client,
                    JiraOperation::Transition {
                        key: p.key,
                        transition_id: p.transition_id,
                        update: Some(
                            json!({
                                    "comment": [{
                                        "add": {
                                            "body": {
                                                "type": "doc",
                                                "version": 1,
                                                "content": [{
                                                    "type": "paragraph",
                                                    "content": [{
                                                        "type": "text",
                                                        "text": format!(
                                                            "[runinator-operation:{operation_key}]"
                                                        )
                                                    }]
                                                }]
                                            }
                                        }
                                    }]
                            })
                            .into(),
                        ),
                    },
                    "jira ensure transition request failed",
                )?;
                return Ok(TaskExecutionResult {
                    message: Some("jira issue transitioned".into()),
                    output_json: Some(json!({
                        "changed": true,
                        "operation_key": operation_key,
                        "status": status,
                        "response": transition
                    })),
                    chunks: Vec::new(),
                    artifacts: Vec::new(),
                });
            }
            "poll_status" | "poll" => {
                let p: JiraIssueKeyParams = parse_params(&request)?;
                jira_call(
                    &client,
                    JiraOperation::Issue {
                        key: p.key,
                        fields: None,
                    },
                    "jira poll request failed",
                )?
            }
            other => {
                return Err(UNSUPPORTED_ACTION.error(other));
            }
        };
        json_response(response)
    }
}

fn jira_call(
    client: &JiraClient,
    operation: JiraOperation,
    context: &str,
) -> Result<serde_json::Value, SendableError> {
    client
        .execute(operation)
        .map_err(|error| client_error(context, error))
}
