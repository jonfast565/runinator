use std::sync::Arc;

use runinator_models::json;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::Serialize;

use crate::command::run_command;
use crate::params::{
    CleanupParams, CommitParams, PushParams, WorkspaceParams, WorktreeParams, parse_params,
};

#[derive(Serialize)]
struct GitResult {
    stdout: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<String>,
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
                        ParameterMetadata::optional("repo", RuninatorType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("branch", RuninatorType::String),
                        ParameterMetadata::required("path", RuninatorType::String),
                    ])
                    .with_results(worktree_results()),
                ActionMetadata::new("branch", "Get current branch name")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", RuninatorType::String)
                            .with_default(json!(".")),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("commit", "Add and commit all changes")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", RuninatorType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("message", RuninatorType::String),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("push", "Push a branch to a remote")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", RuninatorType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::optional("remote", RuninatorType::String)
                            .with_default(json!("origin")),
                        ParameterMetadata::required("branch", RuninatorType::String),
                        ParameterMetadata::optional("set_upstream", RuninatorType::Boolean)
                            .with_default(json!(true)),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("diff", "Get git diff summary")
                    .with_parameters(vec![
                        ParameterMetadata::optional("workspace", RuninatorType::String)
                            .with_default(json!(".")),
                    ])
                    .with_results(git_results()),
                ActionMetadata::new("cleanup", "Remove git worktree")
                    .with_parameters(vec![
                        ParameterMetadata::optional("repo", RuninatorType::String)
                            .with_default(json!(".")),
                        ParameterMetadata::required("path", RuninatorType::String),
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
        token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let function = request.action_function.as_str();
        let timeout = request.timeout_secs;
        let stdout = match function {
            "create_or_resume_worktree" | "worktree" => {
                let params: WorktreeParams = parse_params(&request)?;
                let repo = params.repo.as_deref().unwrap_or(".");
                let stdout = run_command(
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
                    timeout,
                    &token,
                )?;
                let result = GitResult {
                    stdout,
                    action: function.to_string(),
                    workspace: Some(params.path),
                };
                return Ok(TaskExecutionResult {
                    message: Some(format!("Git action {function} completed")),
                    output_json: serde_json::to_value(result).ok().map(Into::into),
                    chunks: Vec::new(),
                    artifacts: Vec::new(),
                });
            }
            "branch" => {
                let params: WorkspaceParams = parse_params(&request)?;
                let ws = params
                    .workspace
                    .as_deref()
                    .or(params.repo.as_deref())
                    .unwrap_or(".");
                run_command(
                    "git",
                    &["-C", ws, "branch", "--show-current"],
                    timeout,
                    &token,
                )?
            }
            "commit" => {
                let params: CommitParams = parse_params(&request)?;
                let ws = params.workspace.as_deref().unwrap_or(".");
                run_command("git", &["-C", ws, "add", "."], timeout, &token)?;
                run_command(
                    "git",
                    &["-C", ws, "commit", "-m", &params.message],
                    timeout,
                    &token,
                )?
            }
            "push" => {
                let params: PushParams = parse_params(&request)?;
                let ws = params.workspace.as_deref().unwrap_or(".");
                let remote = params.remote.as_deref().unwrap_or("origin");
                if params.set_upstream.unwrap_or(true) {
                    run_command(
                        "git",
                        &["-C", ws, "push", "-u", remote, &params.branch],
                        timeout,
                        &token,
                    )?
                } else {
                    run_command(
                        "git",
                        &["-C", ws, "push", remote, &params.branch],
                        timeout,
                        &token,
                    )?
                }
            }
            "diff" => {
                let params: WorkspaceParams = parse_params(&request)?;
                let ws = params
                    .workspace
                    .as_deref()
                    .or(params.repo.as_deref())
                    .unwrap_or(".");
                run_command("git", &["-C", ws, "diff", "--stat"], timeout, &token)?
            }
            "cleanup" => {
                let params: CleanupParams = parse_params(&request)?;
                let repo = params.repo.as_deref().unwrap_or(".");
                run_command(
                    "git",
                    &["-C", repo, "worktree", "remove", &params.path],
                    timeout,
                    &token,
                )?
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
            workspace: None,
        };
        Ok(TaskExecutionResult {
            message: Some(format!("Git action {function} completed")),
            output_json: serde_json::to_value(result).ok().map(Into::into),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
}

fn git_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("stdout", RuninatorType::String),
        ResultMetadata::new("action", RuninatorType::String),
    ]
}

fn worktree_results() -> Vec<ResultMetadata> {
    let mut results = git_results();
    results.push(ResultMetadata::new("workspace", RuninatorType::String));
    results
}
