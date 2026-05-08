use std::sync::Arc;
use std::time::Duration;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct GitHubProvider;

impl Provider for GitHubProvider {
    fn name(&self) -> String {
        "github".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let token = required(&request.parameters, "token")?;
        let owner = required(&request.parameters, "owner")?;
        let repo = required(&request.parameters, "repo")?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(request.timeout_secs.max(1) as u64))
            .user_agent("runinator")
            .build()?;
        let api = "https://api.github.com";
        let auth = format!("Bearer {token}");
        let function = request.action_function.as_str();
        let response = match function {
            "create_or_update_pr" | "create_pr" => client
                .post(format!("{api}/repos/{owner}/{repo}/pulls"))
                .header("Authorization", &auth)
                .header("Accept", "application/vnd.github+json")
                .json(&json!({
                    "title": required(&request.parameters, "title")?,
                    "head": required(&request.parameters, "head")?,
                    "base": str_param(&request.parameters, "base").unwrap_or("main"),
                    "body": str_param(&request.parameters, "body").unwrap_or("")
                }))
                .send()?,
            "read_reviews" | "reviews" => {
                let pull_number = required(&request.parameters, "pull_number")?;
                client
                    .get(format!(
                        "{api}/repos/{owner}/{repo}/pulls/{pull_number}/reviews"
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            "read_issue_comments" | "comments" => {
                let issue_number = required(&request.parameters, "issue_number")?;
                client
                    .get(format!(
                        "{api}/repos/{owner}/{repo}/issues/{issue_number}/comments"
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            "read_checks" | "checks" => {
                let reference = required(&request.parameters, "ref")?;
                client
                    .get(format!(
                        "{api}/repos/{owner}/{repo}/commits/{reference}/check-runs"
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .send()?
            }
            "dispatch_workflow" | "dispatch" => {
                let workflow_id = required(&request.parameters, "workflow_id")?;
                client
                    .post(format!(
                        "{api}/repos/{owner}/{repo}/actions/workflows/{workflow_id}/dispatches"
                    ))
                    .header("Authorization", &auth)
                    .header("Accept", "application/vnd.github+json")
                    .json(&json!({
                        "ref": required(&request.parameters, "ref")?,
                        "inputs": request.parameters.get("inputs").cloned().unwrap_or_else(|| json!({}))
                    }))
                    .send()?
            }
            "poll_workflow_runs" | "workflow_runs" => client
                .get(format!("{api}/repos/{owner}/{repo}/actions/runs"))
                .header("Authorization", &auth)
                .header("Accept", "application/vnd.github+json")
                .send()?,
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
