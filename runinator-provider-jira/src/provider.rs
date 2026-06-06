use std::sync::Arc;
use std::time::Duration;

use runinator_models::json;
use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};

use crate::error::{UNSUPPORTED_ACTION, http_error, validate_base_url};
use crate::metadata::{base_param, email_param, issue_key_param, jira_results, token_param};
use crate::params::{
    JiraCommentParams, JiraIssueKeyParams, JiraSearchParams, JiraTransitionParams, parse_params,
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
            actions: vec![
                ActionMetadata::new("search", "Search Jira issues using JQL")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        ParameterMetadata::required("jql", RuninatorType::String),
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
                        ParameterMetadata::required("body", RuninatorType::String),
                    ])
                    .with_results(jira_results()),
                ActionMetadata::new("transition", "Transition a Jira issue to a new status")
                    .with_parameters(vec![
                        base_param(),
                        token_param(),
                        email_param(),
                        issue_key_param(),
                        ParameterMetadata::required("transition_id", RuninatorType::String),
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
