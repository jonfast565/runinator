use std::sync::Arc;
use std::time::Duration;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct JiraProvider;

impl Provider for JiraProvider {
    fn name(&self) -> String {
        "jira".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let base = required(&request.parameters, "base_url")?;
        let token = required(&request.parameters, "token")?;
        let email = str_param(&request.parameters, "email");
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(request.timeout_secs.max(1) as u64))
            .build()?;
        let function = request.action_function.as_str();
        let response = match function {
            "search_external_items" | "search" => {
                let jql = required(&request.parameters, "jql")?;
                client
                    .get(format!("{base}/rest/api/3/search"))
                    .query(&[("jql", jql)])
                    .basic_auth(email.unwrap_or_default(), Some(token))
                    .send()?
            }
            "fetch_item" | "fetch" => {
                let key = required(&request.parameters, "key")?;
                client
                    .get(format!("{base}/rest/api/3/issue/{key}"))
                    .basic_auth(email.unwrap_or_default(), Some(token))
                    .send()?
            }
            "add_comment" | "comment" => {
                let key = required(&request.parameters, "key")?;
                let body = required(&request.parameters, "body")?;
                client
                    .post(format!("{base}/rest/api/3/issue/{key}/comment"))
                    .basic_auth(email.unwrap_or_default(), Some(token))
                    .json(&json!({ "body": { "type": "doc", "version": 1, "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": body }] }] } }))
                    .send()?
            }
            "transition_item" | "transition" => {
                let key = required(&request.parameters, "key")?;
                let transition_id = required(&request.parameters, "transition_id")?;
                client
                    .post(format!("{base}/rest/api/3/issue/{key}/transitions"))
                    .basic_auth(email.unwrap_or_default(), Some(token))
                    .json(&json!({ "transition": { "id": transition_id } }))
                    .send()?
            }
            "poll_status" | "poll" => {
                let key = required(&request.parameters, "key")?;
                client
                    .get(format!("{base}/rest/api/3/issue/{key}"))
                    .basic_auth(email.unwrap_or_default(), Some(token))
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

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, SendableError> {
    str_param(value, key).ok_or_else(|| {
        Box::new(RuntimeError::new(
            "provider.missing_parameter".into(),
            format!("Missing required parameter '{key}'"),
        )) as SendableError
    })
}

fn str_param<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests;
