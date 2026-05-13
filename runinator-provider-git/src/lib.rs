use std::process::Command;
use std::sync::Arc;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
struct WorktreeParams {
    repo: Option<String>,
    branch: String,
    path: String,
}

#[derive(Deserialize)]
struct WorkspaceParams {
    workspace: Option<String>,
    repo: Option<String>,
}

#[derive(Deserialize)]
struct CommitParams {
    workspace: Option<String>,
    message: String,
}

#[derive(Deserialize)]
struct CleanupParams {
    repo: Option<String>,
    path: String,
}

#[derive(Deserialize)]
struct PushParams {
    workspace: Option<String>,
    remote: Option<String>,
    branch: String,
    set_upstream: Option<bool>,
}

#[derive(Serialize)]
struct GitResult {
    stdout: String,
    action: String,
}

#[derive(Clone)]
pub struct GitProvider;

impl Provider for GitProvider {
    fn name(&self) -> String {
        "git".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("worktree", "Manage git worktrees")
                    .with_parameters(vec![
                        ParameterMetadata::optional("repo", ParameterValueType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("branch", ParameterValueType::String),
                        ParameterMetadata::required("path", ParameterValueType::String),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("branch", "Get current branch name")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", ParameterValueType::String)
                            .with_default(json!(".")),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("commit", "Add and commit all changes")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", ParameterValueType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("message", ParameterValueType::String),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("push", "Push a branch to a remote")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", ParameterValueType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::optional("remote", ParameterValueType::String)
                            .with_default(json!("origin")),
                        ParameterMetadata::required("branch", ParameterValueType::String),
                        ParameterMetadata::optional("set_upstream", ParameterValueType::Boolean)
                            .with_default(json!(true)),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("diff", "Get git diff summary")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", ParameterValueType::String)
                            .with_default(json!(".")),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("cleanup", "Remove git worktree")
                    .with_parameters(vec![
                        ParameterMetadata::optional("repo", ParameterValueType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("path", ParameterValueType::String),
                    ])
                    .with_results(git_results()),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        let function = request.action_function.as_str();
        let stdout = match function {
            "create_or_resume_worktree" | "worktree" => {
                let params: WorktreeParams = parse_params(&request)?;
                let repo = params.repo.as_deref().unwrap_or(".");
                run_command(
                    "git",
                    &[
                        "-C",
                        repo,
                        "worktree",
                        "add",
                        "-B",
                        &params.branch,
                        &params.path,
                    ],
                )?
            }
            "branch" => {
                let params: WorkspaceParams = parse_params(&request)?;
                let ws = params
                    .workspace
                    .as_deref()
                    .or(params.repo.as_deref())
                    .unwrap_or(".");
                run_command("git", &["-C", ws, "branch", "--show-current"])?
            }
            "commit" => {
                let params: CommitParams = parse_params(&request)?;
                let ws = params.workspace.as_deref().unwrap_or(".");
                run_command("git", &["-C", ws, "add", "."])?;
                run_command("git", &["-C", ws, "commit", "-m", &params.message])?
            }
            "push" => {
                let params: PushParams = parse_params(&request)?;
                let ws = params.workspace.as_deref().unwrap_or(".");
                let remote = params.remote.as_deref().unwrap_or("origin");
                if params.set_upstream.unwrap_or(true) {
                    run_command("git", &["-C", ws, "push", "-u", remote, &params.branch])?
                } else {
                    run_command("git", &["-C", ws, "push", remote, &params.branch])?
                }
            }
            "diff" => {
                let params: WorkspaceParams = parse_params(&request)?;
                let ws = params
                    .workspace
                    .as_deref()
                    .or(params.repo.as_deref())
                    .unwrap_or(".");
                run_command("git", &["-C", ws, "diff", "--stat"])?
            }
            "cleanup" => {
                let params: CleanupParams = parse_params(&request)?;
                let repo = params.repo.as_deref().unwrap_or(".");
                run_command("git", &["-C", repo, "worktree", "remove", &params.path])?
            }
            other => {
                return Err(Box::new(RuntimeError::new(
                    "git.unsupported_action".into(),
                    format!("Unsupported Git action {other}"),
                )));
            }
        };
        let result = GitResult {
            stdout,
            action: function.to_string(),
        };
        Ok(TaskExecutionResult {
            message: Some(format!("Git action {function} completed")),
            output_json: serde_json::to_value(result).ok(),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

fn parse_params<T: serde::de::DeserializeOwned>(
    request: &ProviderExecutionRequest,
) -> Result<T, SendableError> {
    serde_json::from_value(request.parameters.clone()).map_err(|e| {
        Box::new(RuntimeError::new(
            "git.invalid_params".into(),
            e.to_string(),
        )) as SendableError
    })
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

fn git_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("stdout", ParameterValueType::String),
        ResultMetadata::new("action", ParameterValueType::String),
    ]
}

#[cfg(test)]
mod tests;
