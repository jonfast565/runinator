use std::{
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct JiraProvider;

#[derive(Clone)]
pub struct GitHubProvider;

#[derive(Clone)]
pub struct GitProvider;

#[derive(Clone)]
pub struct AiCommandProvider;

#[derive(Clone)]
pub struct ApprovalProvider;

impl Provider for JiraProvider {
    fn name(&self) -> String {
        "jira".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        execute_jira(request)
    }
}

impl Provider for GitHubProvider {
    fn name(&self) -> String {
        "github".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        execute_github(request)
    }
}

impl Provider for GitProvider {
    fn name(&self) -> String {
        "git".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        execute_git(request)
    }
}

impl Provider for AiCommandProvider {
    fn name(&self) -> String {
        "ai-command".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        execute_ai_command(request)
    }
}

impl Provider for ApprovalProvider {
    fn name(&self) -> String {
        "approval".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        Ok(TaskExecutionResult {
            message: Some("Approval request prepared".into()),
            output_json: Some(json!({
                "approval_type": str_param(&request.parameters, "approval_type").unwrap_or("generic"),
                "prompt": str_param(&request.parameters, "prompt").unwrap_or("Approval required"),
                "metadata": request.parameters,
            })),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

fn execute_jira(request: ProviderExecutionRequest) -> Result<TaskExecutionResult, SendableError> {
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
            return runtime_err(
                "jira.unsupported_action",
                format!("Unsupported Jira action {other}"),
            );
        }
    };
    json_response("jira", response)
}

fn execute_github(request: ProviderExecutionRequest) -> Result<TaskExecutionResult, SendableError> {
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
            return runtime_err(
                "github.unsupported_action",
                format!("Unsupported GitHub action {other}"),
            );
        }
    };
    json_response("github", response)
}

fn execute_git(request: ProviderExecutionRequest) -> Result<TaskExecutionResult, SendableError> {
    let function = request.action_function.as_str();
    let repo = str_param(&request.parameters, "repo").unwrap_or(".");
    let workspace = str_param(&request.parameters, "workspace").unwrap_or(repo);
    let output = match function {
        "create_or_resume_worktree" | "worktree" => {
            let branch = required(&request.parameters, "branch")?;
            let path = required(&request.parameters, "path")?;
            run_command("git", &["-C", repo, "worktree", "add", "-B", branch, path])?
        }
        "branch" => run_command("git", &["-C", workspace, "branch", "--show-current"])?,
        "commit" => {
            let message = required(&request.parameters, "message")?;
            run_command("git", &["-C", workspace, "add", "."])?;
            run_command("git", &["-C", workspace, "commit", "-m", message])?
        }
        "diff" => run_command("git", &["-C", workspace, "diff", "--stat"])?,
        "cleanup" => {
            let path = required(&request.parameters, "path")?;
            run_command("git", &["-C", repo, "worktree", "remove", path])?
        }
        other => {
            return runtime_err(
                "git.unsupported_action",
                format!("Unsupported Git action {other}"),
            );
        }
    };
    Ok(TaskExecutionResult {
        message: Some(format!("Git action {function} completed")),
        output_json: Some(json!({ "stdout": output, "action": function })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn execute_ai_command(
    request: ProviderExecutionRequest,
) -> Result<TaskExecutionResult, SendableError> {
    let command = required(&request.parameters, "command")?;
    let input = request
        .parameters
        .get("input")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(serde_json::to_string(&input)?.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return runtime_err(
            "ai_command.nonzero_exit",
            String::from_utf8_lossy(&output.stderr).to_string(),
        );
    }
    let stdout = String::from_utf8(output.stdout)?;
    let parsed: Value = serde_json::from_str(&stdout).map_err(|err| {
        RuntimeError::new(
            "ai_command.invalid_json".into(),
            format!("AI command stdout must be JSON: {err}"),
        )
    })?;
    Ok(TaskExecutionResult {
        message: Some("AI command completed".into()),
        output_json: Some(parsed),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn json_response(
    provider: &str,
    response: reqwest::blocking::Response,
) -> Result<TaskExecutionResult, SendableError> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return runtime_err(
            format!("{provider}.http_error"),
            format!("HTTP {status}: {text}"),
        );
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

fn run_command(program: &str, args: &[&str]) -> Result<String, SendableError> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return runtime_err(
            "command.nonzero_exit",
            String::from_utf8_lossy(&output.stderr).to_string(),
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

fn runtime_err<T>(code: impl Into<String>, message: impl Into<String>) -> Result<T, SendableError> {
    Err(Box::new(RuntimeError::new(code.into(), message.into())))
}
