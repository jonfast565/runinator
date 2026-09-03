use std::sync::Arc;
use std::time::Duration;

use runinator_models::json;
use runinator_models::{
    errors::SendableError,
    orchestration::DeliverySemantics,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};

use crate::comments::{fetch_all_comments, jira_fetch_comments, render_comment_body};
use crate::error::{MISSING_OPERATION_KEY, UNSUPPORTED_ACTION, http_error, validate_base_url};
use crate::metadata::{
    base_param, comments_results, email_param, issue_key_param, jira_results, token_param,
};
use crate::params::{
    JiraCommentParams, JiraCommentsParams, JiraEnsureCommentParams, JiraEnsureTransitionParams,
    JiraIssueKeyParams, JiraSearchParams, JiraTransitionParams, parse_params,
};
use crate::response::{json_response, response_json};
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
            actions: vec![
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
            ],
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
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(request.timeout_secs.max(1) as u64))
            .build()
            .map_err(|e| http_error("jira client build failed", e))?;
        let function = request.action_function.as_str();
        let response = match function {
            "search_external_items" | "search" => {
                let p: JiraSearchParams = parse_params(&request)?;
                return jira_search_all(&client, &p);
            }
            "fetch_item" | "fetch" => {
                let p: JiraIssueKeyParams = parse_params(&request)?;
                validate_base_url(&p.base.base_url)?;
                client
                    .get(format!("{}/rest/api/3/issue/{}", p.base.base_url, p.key))
                    .basic_auth(
                        p.base.email.as_deref().unwrap_or_default(),
                        Some(&p.base.token),
                    )
                    .send()
                    .map_err(|e| http_error("jira fetch request failed", e))?
            }
            "add_comment" | "comment" => {
                let p: JiraCommentParams = parse_params(&request)?;
                validate_base_url(&p.base.base_url)?;
                client
                    .post(format!("{}/rest/api/3/issue/{}/comment", p.base.base_url, p.key))
                    .basic_auth(p.base.email.as_deref().unwrap_or_default(), Some(&p.base.token))
                    .json(&json!({ "body": { "type": "doc", "version": 1, "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": p.body }] }] } }))
                    .send()
                    .map_err(|e| http_error("jira comment request failed", e))?
            }
            "ensure_comment" => {
                let p: JiraEnsureCommentParams = parse_params(&request)?;
                validate_base_url(&p.base.base_url)?;
                let operation_key = p
                    .operation_key
                    .or_else(|| request.idempotency_key.clone())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| MISSING_OPERATION_KEY.bare())?;
                let marker = format!("[runinator-operation:{operation_key}]");
                let base = p.base.base_url.trim_end_matches('/');
                let auth_user = p.base.email.as_deref().unwrap_or_default();
                let comments = fetch_all_comments(&client, base, auth_user, &p.base.token, &p.key)?;
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
                let comment = response_json(
                    client
                        .post(format!("{base}/rest/api/3/issue/{}/comment", p.key))
                        .basic_auth(auth_user, Some(&p.base.token))
                        .json(&json!({
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
                        }))
                        .send()
                        .map_err(|e| http_error("jira ensure comment request failed", e))?,
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
                validate_base_url(&p.base.base_url)?;
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
                    .send()
                    .map_err(|e| http_error("jira transition request failed", e))?
            }
            "ensure_transition" => {
                let p: JiraEnsureTransitionParams = parse_params(&request)?;
                validate_base_url(&p.base.base_url)?;
                let operation_key = p
                    .operation_key
                    .or_else(|| request.idempotency_key.clone())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| MISSING_OPERATION_KEY.bare())?;
                let base = p.base.base_url.trim_end_matches('/');
                let auth_user = p.base.email.as_deref().unwrap_or_default();
                let issue = response_json(
                    client
                        .get(format!("{base}/rest/api/3/issue/{}", p.key))
                        .basic_auth(auth_user, Some(&p.base.token))
                        .query(&[("fields", "status")])
                        .send()
                        .map_err(|e| http_error("jira status reconciliation failed", e))?,
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
                let transition = response_json(
                    client
                        .post(format!("{base}/rest/api/3/issue/{}/transitions", p.key))
                        .basic_auth(auth_user, Some(&p.base.token))
                        .json(&json!({
                            "transition": { "id": p.transition_id },
                            "update": {
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
                            }
                        }))
                        .send()
                        .map_err(|e| http_error("jira ensure transition request failed", e))?,
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
                validate_base_url(&p.base.base_url)?;
                client
                    .get(format!("{}/rest/api/3/issue/{}", p.base.base_url, p.key))
                    .basic_auth(
                        p.base.email.as_deref().unwrap_or_default(),
                        Some(&p.base.token),
                    )
                    .send()
                    .map_err(|e| http_error("jira poll request failed", e))?
            }
            other => {
                return Err(UNSUPPORTED_ACTION.error(other));
            }
        };
        json_response(response)
    }
}
