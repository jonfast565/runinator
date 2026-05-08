use std::process::Command;
use std::sync::Arc;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct GitProvider;

impl Provider for GitProvider {
    fn name(&self) -> String {
        "git".into()
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
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
                return Err(Box::new(RuntimeError::new(
                    "git.unsupported_action".into(),
                    format!("Unsupported Git action {other}"),
                )));
            }
        };
        Ok(TaskExecutionResult {
            message: Some(format!("Git action {function} completed")),
            output_json: Some(json!({ "stdout": output, "action": function })),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

fn run_command(program: &str, args: &[&str]) -> Result<String, SendableError> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(Box::new(RuntimeError::new(
            "command.nonzero_exit".into(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )));
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

#[cfg(test)]
mod tests;
